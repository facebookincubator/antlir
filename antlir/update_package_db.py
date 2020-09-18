#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
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

Absent `--no-update-existing`, this CLI updates the "how to fetch" info
dicts for all package/tag pairs in the specified `--db`.  This is equivalent
to passing `--replace package tag '{}'` for each item in the DB.

Other kinds of updates to the DB can be made via `--replace` and `--create`.
Pay careful attention to the description of their `OPTIONS` parameter.

"""
import argparse
import copy
import hashlib
import json
import os
from enum import Enum
from typing import (
    Callable,
    ContextManager,
    Dict,
    Iterator,
    List,
    NamedTuple,
    Optional,
    Tuple,
)

from .common import get_file_logger, init_logging
from .fs_utils import Path, populate_temp_dir_and_rename


_GENERATED = "@" + "generated"
_JSON = ".json"

log = get_file_logger(__file__)

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
#     updater --db path --create package tag '{...}' --no-update-existing
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
GetDbInfoFn = Callable[[Package, Tag, DbUpdateOptions], Optional[DbInfo]]
# The in-memory representation of the package DB.
PackageTagDb = Dict[Package, Dict[Tag, DbInfo]]


class UpdateAction(Enum):
    CREATE = "create"
    REPLACE = "replace"


class PackageDbUpdate(NamedTuple):
    action: UpdateAction
    options: DbUpdateOptions


class InvalidCommandError(Exception):  # noqa: B903
    def __init__(self, pkg: Package, tag: Tag):
        self.pkg = pkg
        self.tag = tag


class PackageExistsError(InvalidCommandError):
    def __init__(self, pkg: Package, tag: Tag, db_info: DbInfo):
        super().__init__(pkg, tag)
        self.db_info = db_info

    def __str__(self):
        return (
            "Attempting to create a package:tag that already exists in the DB: "
            f"{self.pkg}:{self.tag}"
        )


class PackageDoesNotExistError(InvalidCommandError):
    def __str__(self):
        return (
            "Attempting to replace a package:tag that does not exist in the "
            f"DB: {self.pkg}:{self.tag}"
        )


# `--replace` and `--create` opts are parsed into this form.
ExplicitUpdates = Dict[Package, Dict[Tag, PackageDbUpdate]]


def _with_generated_header_impl(s, token, how_to_generate):
    return f"# {_GENERATED} {token}\n# Update via `{how_to_generate}`\n" + s


def _with_generated_header(contents, how_to_generate):
    # We'll inject the MD5 of the contents of the file into the header.
    # Lint complains if the MD5 in the header does not match the contents.
    # This is not a security measure.  It is only intended to discourage
    # people from manually resolving merge conflicts, which is error-prone
    # and can break trunk if a bad merge is accidentally committed.
    hex_hash = hashlib.md5(
        _with_generated_header_impl(
            contents,
            # This is the same magic value that lint uses, yay.
            "<<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>",
            how_to_generate,
        ).encode()
    ).hexdigest()
    return _with_generated_header_impl(
        contents, f"SignedSource<<{hex_hash}>>", how_to_generate
    )


def _write_json_dir_db(db: PackageTagDb, path: Path, how_to_generate: str):
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


def _read_generated_header(infile):
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


def _validate_updates(existing_db: PackageTagDb, pkg_updates: ExplicitUpdates):
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


def _get_updated_db(
    *,
    existing_db: PackageTagDb,
    get_db_info_fn: GetDbInfoFn,
    update_existing: bool,
    pkg_updates: ExplicitUpdates,
) -> PackageTagDb:
    _validate_updates(existing_db, pkg_updates)
    # Extract option dicts from the `PackageDbUpdate` objects that were
    # provided, as this is the format of the existing DB that gets extracted
    pkg_to_update_dcts = {
        pkg: {tag: update.options for tag, update in tag_to_update.items()}
        for pkg, tag_to_update in pkg_updates.items()
    }
    if update_existing:
        # We're updating the entire DB so we start with a clean slate
        db_to_update = {}
        updates_to_apply = {
            # These are existing DB entries for which we have to fetch and
            # resolve new values.
            **{
                pkg: {tag: {} for tag in tag_to_info}
                for pkg, tag_to_info in existing_db.items()
            },
            **pkg_to_update_dcts,
        }
    else:
        # If we are not updating the existing DB (i.e. explicit updates only),
        # start with a copy of the existing DB, and replace info as we go.
        db_to_update = copy.deepcopy(existing_db)
        updates_to_apply = pkg_to_update_dcts

    for pkg, tag_to_update in updates_to_apply.items():
        new_tag_to_info = db_to_update.setdefault(pkg, {})
        for tag, update_opts in tag_to_update.items():
            log.info(f"Querying {pkg}:{tag} with options {update_opts}")
            info = get_db_info_fn(pkg, tag, update_opts)
            if info is None:
                log.info(
                    f"Empty info returned for {pkg}:{tag} - not including in DB"
                )
                new_tag_to_info.pop(tag, None)
            else:
                new_tag_to_info[tag] = info
                log.info(f"New info for {pkg}:{tag} -> {info}")
    return db_to_update


def update_package_db(
    *,
    db_path: Path,
    how_to_generate: str,
    get_db_info_factory: ContextManager[GetDbInfoFn],
    out_db_path: Optional[Path] = None,
    update_existing: bool = True,
    pkg_updates: Optional[ExplicitUpdates] = None,
):
    with get_db_info_factory as get_db_info:
        _write_json_dir_db(
            db=_get_updated_db(
                existing_db=_read_json_dir_db(db_path),
                get_db_info_fn=get_db_info,
                update_existing=update_existing,
                pkg_updates=pkg_updates or {},
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
        for package, tag, opts_json in action_updates:
            opts = json.loads(opts_json)
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


def _parse_args(argv, *, overview_doc, options_doc):
    parser = argparse.ArgumentParser(
        description=__doc__ + overview_doc,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--db",
        type=Path.from_argparse,
        required=True,
        help="Path to the database to update",
    )
    parser.add_argument(
        "--out-db",
        type=Path.from_argparse,
        help="Path for the updated database (defaults to `--db`)",
    )
    parser.add_argument(
        "--no-update-existing",
        action="store_false",
        dest="update_existing",
        help="Only update package / tag pairs set via --create or --replace",
    )

    for action, doc in [
        (
            "create",
            "Ensures this PACKAGE/TAG pair will be updated in the DB, "
            f"even if `--no-update-existing` was passed. {options_doc} "
            "Asserts that the PACKAGE/TAG pair was NOT already in the DB. ",
        ),
        (
            "replace",
            "Just like `--create`, but asserts that the PACKAGE/TAG pair "
            "already exists in the DB.",
        ),
    ]:
        parser.add_argument(
            "--" + action,
            action="append",
            default=[],
            nargs=3,
            metavar=("PACKAGE", "TAG", "OPTIONS"),
            help=doc,
        )
    parser.add_argument(  # Pass this to `init_logging`
        "--debug", action="store_true", help="Enable verbose logging"
    )
    return Path.parse_args(parser, argv)


def main_cli(
    argv: List[str],
    get_db_info_factory: ContextManager[Iterator[GetDbInfoFn]],
    *,
    how_to_generate: str,
    overview_doc: str,
    options_doc: str,
):
    """
    Implements the "update DB" CLI using your custom logic for obtaiing
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
    args = _parse_args(argv, overview_doc=overview_doc, options_doc=options_doc)
    init_logging(debug=args.debug)
    update_package_db(
        db_path=args.db,
        how_to_generate=how_to_generate,
        get_db_info_factory=get_db_info_factory,
        out_db_path=args.out_db,
        update_existing=args.update_existing,
        pkg_updates=_parse_update_args(args.create, args.replace),
    )
