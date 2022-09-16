#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# The following docblock describes the CLI of the binary that can be built
# from this library.  To create such a binary, you will need to call `main`
# -- refer to its docblock for usage.
"""
This CLI helps maintain package databases used by the Buck macro functions
`fetched_package_layers_from_json_dir_db` to expose externally fetched packages
as `image.layer` targets.  Read the documentation in `fetched_package_layer.bzl`
for more details.

You will typically not run this manually, except to register a new package.
For that usage, refer to the instructions in the TARGETS file of the project
that contains your DB.

Routine package updates are done automatically by a periodic job that
commits to the repo the results of running this with just `--db path/to/db`.

With `--update-all` set, this CLI updates the "how to fetch" info
dicts for all package/tag pairs in the specified `--db`.  This is equivalent
to passing `--replace package tag '{}'` for each item in the DB.

Other kinds of updates to the DB can be made via `--replace` and `--create`.
Pay careful attention to the description of their `OPTIONS` parameter.

"""
import argparse
import asyncio
import copy
import json
import os
from collections import defaultdict
from enum import Enum
from typing import (
    Any,
    Awaitable,
    Callable,
    ContextManager,
    Dict,
    Iterable,
    Iterator,
    List,
    NamedTuple,
    Optional,
    Tuple,
    Type,
)

from antlir.common import get_logger, init_logging
from antlir.fs_utils import Path, populate_temp_dir_and_rename
from antlir.signed_source import sign_source, signed_source_sigil


_GENERATED: str = "@" + "generated"
_JSON = ".json"

log = get_logger()

Package = str
Tag = str
# For each (package, tag), the DB stores this opaque dictionary, which tells
# `_PackageFetcherInfo.fetch_package` how to download the package.
DbInfo = Dict[str, str]
# This opaque map is passed from the command-line (via `--create` or
# `--replace`) down to the implementation in `GetDbInfoFn`, and helps
# it to decide how to obtain the `DbInfo` for the given (package, tag).
DbUpdateOptions = Dict[str, str]
# The simplest implementation of `GetDbInfoFn` is the identity:
#
#     def get_db_info(package, tag, options):
#         return options
#
# In this set-up, all DB maintenance will be done in this fashion:
#
#     updater --db path --create package tag '{...}' --no-update-all
#
# In other words, the repo is the sole source of truth for what package
# instances a given tag refers to.
#
# If the source of truth is external, then this function can instead query
# the external source of truth, e.g. for a RESTful API;
#
#     def get_external_db_info(package, tag, options):
#         return requests.get(
#             'http://packages.example.com/package',
#             params={'tag': tag, **options},
#         ).json()
#
# If the external source of truth requires no options, the entire DB can
# be updated from the CLI via:
#
#     updater --db path
#
# Note if None is returned, the given package:tag pair will be deleted
GetDbInfoRet = Tuple[Package, Tag, Optional[DbInfo]]
GetDbInfoFn = Callable[[Package, Tag, DbUpdateOptions], Awaitable[GetDbInfoRet]]
# The in-memory representation of the package DB.
PackageTagDb = Dict[Package, Dict[Tag, DbInfo]]


class UpdateAction(Enum):
    CREATE = "create"
    REPLACE = "replace"


class PackageDbUpdate(NamedTuple):
    action: UpdateAction
    options: DbUpdateOptions


class InvalidCommandError(Exception):  # noqa: B903
    def __init__(self, pkg: Package, tag: Tag) -> None:
        self.pkg = pkg
        self.tag = tag


class PackageExistsError(InvalidCommandError):
    def __init__(self, pkg: Package, tag: Tag, db_info: DbInfo) -> None:
        super().__init__(pkg, tag)
        self.db_info = db_info

    def __str__(self) -> str:
        return (
            "Attempting to create a package:tag that already exists in the DB: "
            f"{self.pkg}:{self.tag}"
        )


class PackageDoesNotExistError(InvalidCommandError):
    def __str__(self) -> str:
        return (
            "Attempting to replace a package:tag that does not exist in the "
            f"DB: {self.pkg}:{self.tag}"
        )


# `--replace` and `--create` opts are parsed into this form.
ExplicitUpdates = Dict[Package, Dict[Tag, PackageDbUpdate]]


def _with_generated_header_impl(s, token, how_to_generate) -> str:
    return f"# {_GENERATED} {token}\n# Update via `{how_to_generate}`\n" + s


def _with_generated_header(contents, how_to_generate) -> str:
    return sign_source(
        _with_generated_header_impl(
            contents,
            signed_source_sigil(),
            how_to_generate,
        )
    )


def _write_json_dir_db(
    db: PackageTagDb, path: Path, how_to_generate: str
) -> None:
    # pyre-fixme[16]: `Path` has no attribute `__enter__`.
    with populate_temp_dir_and_rename(path, overwrite=True) as td:
        for package, tag_to_info in db.items():
            os.mkdir(td / package)
            for tag, info in tag_to_info.items():
                with open(td / package / (tag + _JSON), "w") as outf:
                    outf.write(
                        _with_generated_header(
                            json.dumps(info, sort_keys=True, indent=4) + "\n",
                            how_to_generate,
                        )
                    )


def _read_generated_header(infile) -> None:
    generated_header = infile.readline()
    # Note: We don't verify the signature verification on read, since it's
    # only point is to be checked by lint (doc on `_with_generated_header`).
    assert _GENERATED in generated_header, generated_header
    infile.readline()  # Our header is 2 lines, the second one is ignored


def _read_json_dir_db(path: Path) -> PackageTagDb:
    db = {}
    for package in path.listdir():
        tag_to_info = db.setdefault(package.decode(), {})
        for tag_json in (path / package).listdir():
            tag_json = tag_json.decode()
            assert tag_json.endswith(_JSON), (path, package, tag_json)
            with open(path / package / tag_json) as infile:
                _read_generated_header(infile)
                tag_to_info[tag_json[: -len(_JSON)]] = json.load(infile)
    return db


def _validate_updates(
    existing_db: PackageTagDb, pkg_updates: ExplicitUpdates
) -> None:
    """Perform validations on any updates that were provided:
    - Don't create package:tag pairs that already exist
    - Don't replace package:tag pairs that don't already exist
    """
    for pkg, tag_to_update in pkg_updates.items():
        for tag, update in tag_to_update.items():
            curr_info = existing_db.get(pkg, {})
            if update.action == UpdateAction.CREATE and tag in curr_info:
                raise PackageExistsError(pkg, tag, curr_info[tag])
            elif update.action == UpdateAction.REPLACE and tag not in curr_info:
                raise PackageDoesNotExistError(pkg, tag)


async def _get_db_info_bounded(
    sem: asyncio.Semaphore,
    pkg: str,
    tag: str,
    new_opts: DbInfo,
    # Accept existing opts in the DB so we can keep the value unchanged if info
    # retrieval raised a skippable exception
    exist_opts: DbInfo,
    get_db_info_fn: GetDbInfoFn,
    is_exception_skippable: Optional[Callable[[Exception], bool]] = None,
) -> GetDbInfoRet:
    async with sem:
        try:
            return await get_db_info_fn(pkg, tag, new_opts)
        except Exception as e:
            if is_exception_skippable and is_exception_skippable(e):
                log.warning(f"Caught skippable exception, continuing: {e}")
                return pkg, tag, exist_opts
            raise e


async def _get_updated_db(
    *,
    existing_db: PackageTagDb,
    get_db_info_fn: GetDbInfoFn,
    update_all: bool,
    pkg_updates: ExplicitUpdates,
    is_exception_skippable: Optional[Callable[[Exception], bool]] = None,
) -> PackageTagDb:
    _validate_updates(existing_db, pkg_updates)
    # Extract option dicts from the `PackageDbUpdate` objects that were
    # provided, as this is the format of the existing DB that gets extracted
    pkg_to_update_dcts = {
        pkg: {tag: update.options for tag, update in tag_to_update.items()}
        for pkg, tag_to_update in pkg_updates.items()
    }
    if update_all:
        # We're updating the entire DB so we start with a clean slate
        db_to_update = defaultdict(dict)
        updates_to_apply = {
            # These are existing DB entries for which we have to fetch and
            # resolve new values.
            **{
                # Empty dict for 'options' as we want them to be re-populated
                pkg: {tag: {} for tag in tag_to_info.keys()}
                for pkg, tag_to_info in existing_db.items()
            },
            **pkg_to_update_dcts,
        }
    else:
        # If we are not updating the existing DB (i.e. explicit updates only),
        # start with a copy of the existing DB, and replace info as we go.
        db_to_update = defaultdict(dict, copy.deepcopy(existing_db))
        updates_to_apply = pkg_to_update_dcts
    # Use a semaphore to limit concurrency and ensure we're not issuing 1000s of
    # simultaneous requests for large DBs
    sem = asyncio.Semaphore(32)
    futures = []
    for pkg, tag_to_update_opts in updates_to_apply.items():
        for tag, new_opts in tag_to_update_opts.items():
            exist_opts = {}
            if pkg in existing_db and tag in existing_db[pkg]:
                exist_opts = existing_db[pkg][tag]
            fut = _get_db_info_bounded(
                sem=sem,
                pkg=pkg,
                tag=tag,
                new_opts=new_opts,
                exist_opts=exist_opts,
                get_db_info_fn=get_db_info_fn,
                is_exception_skippable=is_exception_skippable,
            )
            futures.append(fut)
    for f in asyncio.as_completed(futures):
        pkg, tag, new_maybe_info = await f
        if new_maybe_info is None:
            log.warning(
                f"Empty info returned for {pkg}:{tag} - not including in DB"
            )
            db_to_update[pkg].pop(tag, None)
        else:
            log.info(f"New info for {pkg}:{tag} -> {new_maybe_info}")
            db_to_update[pkg][tag] = new_maybe_info
    return db_to_update


async def update_package_db(
    *,
    db_path: Path,
    how_to_generate: str,
    get_db_info_factory: ContextManager[GetDbInfoFn],
    out_db_path: Optional[Path] = None,
    update_all: bool = True,
    pkg_updates: Optional[ExplicitUpdates] = None,
    is_exception_skippable: Optional[Callable[[Exception], bool]] = None,
) -> None:
    with get_db_info_factory as get_db_info:
        _write_json_dir_db(
            db=await _get_updated_db(
                existing_db=_read_json_dir_db(db_path),
                get_db_info_fn=get_db_info,
                update_all=update_all,
                pkg_updates=pkg_updates or {},
                is_exception_skippable=is_exception_skippable,
            ),
            path=out_db_path or db_path,
            how_to_generate=how_to_generate,
        )


UpdateArgs = List[Tuple[Package, Tag, str]]


def _parse_update_args(
    creates: UpdateArgs, replaces: UpdateArgs
) -> ExplicitUpdates:
    """Parses the provided update arg lists and creates corresponding
    PackageDbUpdate tuples for each action.

    Also validates that a given pkg:tag doesn't have multiple updates specified.
    Note that this is a default guarantee provided by our data model for updates
    (mapping of pkg to tag to update), but checking explcitly here allows us to
    provide clear error messages.
    """
    pkg_updates = {}
    for action_updates, action in zip(
        (creates, replaces), (UpdateAction.CREATE, UpdateAction.REPLACE)
    ):
        for package, tag, *opts_json in action_updates:
            if len(opts_json) > 1:
                raise RuntimeError(
                    f"Invalid options specified for action {action}: "
                    f"{opts_json}"
                )
            opts = json.loads(opts_json[0]) if opts_json else {}
            if tag in pkg_updates.setdefault(package, {}):
                existing_up = pkg_updates[package][tag]
                raise RuntimeError(
                    f'Multiple updates specified for "{package}:{tag}": '
                    f'"{action.value}" with {opts} and '
                    f'"{existing_up.action.value}" with {existing_up.options}'
                )
            pkg_updates[package][tag] = PackageDbUpdate(
                action=action, options=opts
            )
    return pkg_updates


def _parse_args(
    argv: List[str],
    *,
    overview_doc: str,
    options_doc: str,
    defaults: Dict[str, Any],
    show_oss_overview_doc: bool = True,
):
    parser = argparse.ArgumentParser(
        description=(__doc__ + overview_doc)
        if show_oss_overview_doc
        else overview_doc,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--db",
        type=Path.from_argparse,
        required="db" not in defaults,
        help="Path to the database to update",
        default=defaults.get("db"),
    )
    parser.add_argument(
        "--out-db",
        type=Path.from_argparse,
        help="Path for the updated database (defaults to `--db`)",
        default=defaults.get("out_db"),
    )
    parser.add_argument(
        "--update-all",
        action="store_true",
        help="Update all packages in the DB regardless of whether an explicit "
        "action was provided.",
        default=defaults.get("update_all"),
    )

    for action, doc in [
        (
            "create",
            "Adds the specified 'PACKAGE TAG' pair to the DB, and fails if an "
            f"entry already exists. {options_doc}",
        ),
        (
            "replace",
            "Just like `--create`, but asserts that the 'PACKAGE TAG' pair "
            "already exists in the DB.",
        ),
    ]:
        parser.add_argument(
            "--" + action,
            action="append",
            default=defaults.get(action, []),
            nargs="+",
            metavar="PACKAGE TAG [OPTIONS]",
            help=doc,
        )
    parser.add_argument(  # Pass this to `init_logging`
        "--debug",
        action="store_true",
        help="Enable verbose logging",
        default=defaults.get("debug"),
    )
    return Path.parse_args(parser, argv)


async def main_cli(
    argv: List[str],
    get_db_info_factory: ContextManager[Iterator[GetDbInfoFn]],
    *,
    how_to_generate: str,
    overview_doc: str,
    options_doc: str,
    defaults: Optional[Dict[str, Any]] = None,
    show_oss_overview_doc: bool = True,
) -> None:
    """
    Implements the "update DB" CLI using your custom logic for obtaining
    `DbInfo` objects for package:tag pairs.

    `get_db_info_factory` is a context manager so that it can establish a
    single connection (or pool) to an external service, and reuse it for all
    `GetDbInfoFn` queries.

    To implement "identity" example from the `GetDbInfoFn` docblock above:

        main(
            sys.argv[1:],
            contextlib.nullcontext(lambda _pkg, _tag, opts: opts),
            how_to_generate='buck run //your-project:manually-update-db',
            overview_doc='',
            options_doc='OPTIONS are written directly into the DB as '
                'the "how to fetch" info for this PACKAGE/TAG.",
        )

    In reality, you would want your `GetDbInfoFn` to do some schema
    validation, and to check that the "how to fetch" info does actually
    refer to a valid package in your package store.
    """
    args = _parse_args(
        argv,
        overview_doc=overview_doc,
        options_doc=options_doc,
        defaults=defaults or {},
        show_oss_overview_doc=show_oss_overview_doc,
    )
    explicit_updates = _parse_update_args(args.create, args.replace)
    if not (explicit_updates or args.update_all):  # pragma: no cover
        log.warning(
            "No explicit actions provided and --update-all not set; no "
            "work to be done."
        )
    init_logging(debug=args.debug)
    await update_package_db(
        db_path=args.db,
        how_to_generate=how_to_generate,
        # pyre-fixme[6]: Expected `ContextManager[typing.Callable[[str, str,
        #  Dict[str, str]], Awaitable[Tuple[str, str, Optional[Dict[str, str]]]]]]` for
        #  3rd param but got `ContextManager[Iterator[typing.Callable[[str, str,
        #  Dict[str, str]], Awaitable[Tuple[str, str, Optional[Dict[str, str]]]]]]]`.
        get_db_info_factory=get_db_info_factory,
        out_db_path=args.out_db,
        update_all=args.update_all,
        pkg_updates=explicit_updates,
    )
