#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# The following docblock describes the CLI of the binary that can be built
# from this library.  To create such a binary, you will need to call `main`
# -- refer to its docblock for usage.
'''
This CLI helps maintain package databases used by the Buck macro functions
`fetched_package_layers_from_{bzl,json_dir}_db` to expose externally
fetched packages as `image.layer` targets.  Read the documentation in
`fetched_package_layer.bzl` for more details.

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

'''
import argparse
import ast
import copy
import hashlib
import json
import os

from typing import Callable, List, Mapping, Tuple

from fs_image.common import get_file_logger, init_logging
from fs_image.fs_utils import (
    Path, populate_temp_dir_and_rename, populate_temp_file_and_rename,
)

_GENERATED = '@' + 'generated'
_JSON = '.json'
_BZL_DB_PREFIX = 'package_db = '
_INDENT_SPACES = 4

log = get_file_logger(__file__)

Package = str
Tag = str
# For each (package, tag), the DB stores this opaque dictionary, which tells
# `_PackageFetcherInfo.fetch_package` how to download the package.
DbInfo = Mapping[str, str]
# This opaque map is passed from the command-line (via `--create` or
# `--replace`) down to the implementation in `GetDbInfoFn`, and helps
# it to decide how to obtain the `DbInfo` for the given (package, tag).
DbUpdateOptions = Mapping[str, str]
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
GetDbInfoFn = Callable[[Package, Tag, DbUpdateOptions], DbInfo]
# The in-memory representation of the package DB.
PackageTagDb = Mapping[Package, Mapping[Tag, DbInfo]]
# `--replace` and `--create` parameters are parsed into this form.
ExplicitUpdates = Mapping[Package, Mapping[Tag, DbUpdateOptions]]


def _with_generated_header_impl(s, token, how_to_generate):
    return f'# {_GENERATED} {token}\n# Update via `{how_to_generate}`\n' + s


def _with_generated_header(contents, how_to_generate):
    # We'll inject the MD5 of the contents of the file into the header.
    # Lint complains if the MD5 in the header does not match the contents.
    # This is not a security measure.  It is only intended to discourage
    # people from manually resolving merge conflicts, which is error-prone
    # and can break trunk if a bad merge is accidentally committed.
    hex_hash = hashlib.md5(_with_generated_header_impl(
        contents,
        # This is the same magic value that lint uses, yay.
        '<<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>',
        how_to_generate,
    ).encode()).hexdigest()
    return _with_generated_header_impl(
        contents, f'SignedSource<<{hex_hash}>>', how_to_generate,
    )


# TARGETS auto-formatting requires double-quotes and trailing commas, so we
# need our own serializer :/ -- `repr` or JSON won't do.
def _buildifier_repr(x, depth=0, *, is_inline=False):
    indent = ' ' * _INDENT_SPACES * depth
    first_indent = '' if is_inline else indent
    if isinstance(x, str):
        return first_indent + (
            '"' +
            x.encode('unicode_escape').decode('ascii').replace('"', '\\"') +
            '"'
        )
    elif isinstance(x, bool):
        return first_indent + str(x)
    elif isinstance(x, dict):
        return first_indent + '{\n' + ',\n'.join((
            _buildifier_repr(k, depth + 1) + ': ' +
            _buildifier_repr(v, depth + 1, is_inline=True).lstrip(' ')
        ) for k, v in sorted(x.items())) + (',\n' if x else '') + indent + '}'


def _write_bzl_db(db: PackageTagDb, path: Path, how_to_generate: str):
    # NB: This will fail to replace a directory, preventing us from
    # transparently converting JSON to BZL databases without using
    # `--out-db`.  This is fine since the DB paths should look different
    # anyhow (`dirname` vs `filename.bzl`).
    with populate_temp_file_and_rename(path, overwrite=True) as outfile:
        outfile.write(_with_generated_header(
            _BZL_DB_PREFIX + _buildifier_repr(db) + '\n',
            how_to_generate,
        ))


def _write_json_dir_db(db: PackageTagDb, path: Path, how_to_generate: str):
    with populate_temp_dir_and_rename(path, overwrite=True) as td:
        for package, tag_to_info in db.items():
            os.mkdir(td / package)
            for tag, info in tag_to_info.items():
                with open(td / package / (tag + _JSON), 'w') as outf:
                    outf.write(_with_generated_header(
                        json.dumps(info, sort_keys=True, indent=4) + '\n',
                        how_to_generate,
                    ))


_FORMAT_NAME_TO_WRITER = {
    'bzl': _write_bzl_db,
    'json-dir': _write_json_dir_db,
}


def _read_generated_header(infile):
    generated_header = infile.readline()
    # Note: We don't verify the signature verification on read, since it's
    # only point is to be checked by lint (doc on `_with_generated_header`).
    assert _GENERATED in generated_header, generated_header
    infile.readline()  # Our header is 2 lines, the second one is ignored


def _read_bzl_db(path: Path) -> PackageTagDb:
    with open(path) as infile:
        _read_generated_header(infile)
        db_str = infile.read()
    assert db_str.startswith(_BZL_DB_PREFIX)
    return ast.literal_eval(db_str[len(_BZL_DB_PREFIX):])


def _read_json_dir_db(path: Path) -> PackageTagDb:
    db = {}
    for package in os.listdir(path):
        tag_to_info = db.setdefault(package.decode(), {})
        for tag_json in os.listdir(path / package):
            tag_json = tag_json.decode()
            assert tag_json.endswith(_JSON), (path, package, tag_json)
            with open(path / package / tag_json) as infile:
                _read_generated_header(infile)
                tag_to_info[tag_json[:-len(_JSON)]] = json.load(infile)
    return db


def _read_db_and_get_writer(path: Path) -> Tuple[
    PackageTagDb,
    Callable[[PackageTagDb, str], None],
]:
    # We don't need very fancy autodetection at present, and I don't
    # anticipate adding more DB formats soon.
    if os.path.isfile(path):
        return _read_bzl_db(path), _write_bzl_db
    elif os.path.isdir(path):
        return _read_json_dir_db(path), _write_json_dir_db
    raise RuntimeError(f'Bad path {path}')  # pragma: no cover


def _get_updated_db(
    *,
    existing_db: PackageTagDb,
    update_existing: bool,
    create_items: ExplicitUpdates,
    replace_items: ExplicitUpdates,
    get_db_info_fn: GetDbInfoFn,
) -> PackageTagDb:

    # The `updates` map tells us the packages, for which we have to fetch
    # new DB entries.
    updates = {
        package: {tag: {} for tag in tag_to_info}
            for package, tag_to_info in existing_db.items()
    } if update_existing else {}
    # Merge any `ExplictUpdates` into `updates.  "replace" precedes "create"
    # to make us fail if a `--replace` conflicts with a `--create`.
    for explicit_updates in [replace_items, create_items]:
        for package, in_tag_to_update_opts in explicit_updates.items():
            out_tag_to_update_opts = updates.setdefault(package, {})
            for tag, update_opts in in_tag_to_update_opts.items():
                seen_before = (
                    tag in out_tag_to_update_opts or
                    tag in existing_db.get(package, {})
                )
                if explicit_updates is create_items:
                    assert not seen_before, (package, tag)
                elif explicit_updates is replace_items:
                    # Since `replace` is applied first, this will never
                    # erroneously `replace` a conflicting `create`.
                    assert seen_before, (package, tag)
                else:  # pragma: no cover
                    raise AssertionError('Not reached')
                out_tag_to_update_opts[tag] = update_opts

    # If we are not updating the existing DB (i.e. explicit updates only),
    # start with a copy of the existing DB, and replace infos as we go.
    new_db = copy.deepcopy(existing_db) if not update_existing else {}

    # Apply all `updates` to `new_db`
    for package, tag_to_update_opts in updates.items():
        new_tag_to_info = new_db.setdefault(package, {})
        for tag, update_opts in tag_to_update_opts.items():
            log.info(f'Querying {package}:{tag} with options {update_opts}')
            info = get_db_info_fn(package, tag, update_opts)
            new_tag_to_info[tag] = info
            log.info(f'New info for {package}:{tag} -> {info}')

    return new_db


def _parse_updates(
    description: str,
    items: List[Tuple[Package, Tag, str]],
) -> ExplicitUpdates:
    updates = {}
    for package, tag, opts_str in items:
        opts = json.loads(opts_str)
        stored_opts = updates.setdefault(package, {}).setdefault(tag, opts)
        if stored_opts is not opts:  # `!=` would permit duplicates
            # This detects conflicts only within a single update type,
            # `_get_updated_db` detects conflicts between types.
            raise RuntimeError(
                f'Conflicting "{description}" updates for {package} / {tag}: '
                f'{opts} ({id(opts)}) is not {stored_opts} ({id(stored_opts)}.'
            )
    return updates


def _parse_args(argv, *, overview_doc, options_doc):
    parser = argparse.ArgumentParser(
        description=__doc__ + overview_doc,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        '--db', type=Path.from_argparse, required=True,
        help='Path to the database to update',
    )
    parser.add_argument(
        '--out-db', type=Path.from_argparse,
        help='Path for the updated database (defaults to `--db`)',
    )
    parser.add_argument(
        '--out-format',
        choices=set(_FORMAT_NAME_TO_WRITER.keys()),
        help='Which database format to write? (defaults to the auto-detected '
            'input format)',
    )
    parser.add_argument(
        '--no-update-existing', action='store_false', dest='update_existing',
        help='Only update package / tag pairs set via --create or --replace',
    )
    for action, doc in [
        (
            'create',
            'Ensures this PACKAGE/TAG pair will be updated in the DB, '
            f'even if `--no-update-existing` was passed. {options_doc} '
            'Asserts that the PACKAGE/TAG pair was NOT already in the DB. ',
        ),
        (
            'replace',
            'Just like `--create`, but asserts that the PACKAGE/TAG pair '
            'already exists in the DB.',
        ),
    ]:
        parser.add_argument(
            '--' + action, action='append', default=[], nargs=3,
            metavar=('PACKAGE', 'TAG', 'OPTIONS'), help=doc,
        )
    parser.add_argument(  # Pass this to `init_logging`
        '--debug', action='store_true', help='Enable verbose logging',
    )
    return parser.parse_args(argv)


def main(
    argv, get_db_info_factory, *, how_to_generate, overview_doc, options_doc,
):
    '''
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
    '''
    args = _parse_args(
        argv, overview_doc=overview_doc, options_doc=options_doc,
    )
    init_logging(debug=args.debug)
    db, write_db_in_same_format = _read_db_and_get_writer(args.db)
    write_db = _FORMAT_NAME_TO_WRITER[args.out_format] if args.out_format \
        else write_db_in_same_format
    with get_db_info_factory as get_db_info_fn:
        write_db(
            _get_updated_db(
                existing_db=db,
                update_existing=args.update_existing,
                create_items=_parse_updates('create', args.create),
                replace_items=_parse_updates('replace', args.replace),
                get_db_info_fn=get_db_info_fn
            ),
            args.out_db or args.db,
            how_to_generate,
        )
