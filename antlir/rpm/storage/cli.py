#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Uniform command-line interface to the rpm/storage abstraction."
import argparse
from io import BytesIO

from antlir.common import init_logging
from antlir.fs_utils import Path
from antlir.rpm.common import read_chunks

from .storage import Storage


_CHUNK_SIZE = 2 ** 20  # Not too important, anything large enough is fine.


def put(args):
    storage = Storage.from_json(args.storage)
    with storage.writer() as fout:
        for chunk in read_chunks(args.from_file, _CHUNK_SIZE):
            fout.write(chunk)
        args.to_file.write((fout.commit() + "\n").encode())


def get(args):
    storage = Storage.from_json(args.storage)
    with storage.reader(args.storage_id) as fin:
        for chunk in read_chunks(fin, _CHUNK_SIZE):
            args.to_file.write(chunk)


def main(argv, from_file: BytesIO, to_file: BytesIO):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    Storage.add_argparse_arg(
        parser,
        "--storage",
        required=True,
        help="JSON blob for creating a Storage instance.",
    )
    parser.add_argument("--debug", action="store_true", help="Log more?")
    subparsers = parser.add_subparsers(help="Sub-commands have help.")

    parser_get = subparsers.add_parser("get", help="Download blob to stdout")
    parser_get.add_argument("storage_id", help="String of the form KEY:ID")
    parser_get.set_defaults(to_file=to_file)
    parser_get.set_defaults(func=get)

    parser_put = subparsers.add_parser(
        "put", help="Write a blob from stdin, print its ID to stdout"
    )
    parser_put.set_defaults(from_file=from_file)
    parser_put.set_defaults(to_file=to_file)  # For the storage ID
    parser_put.set_defaults(func=put)

    args = Path.parse_args(parser, argv)
    init_logging(debug=args.debug)

    args.func(args)


if __name__ == "__main__":  # pragma: no cover
    import sys

    # pyre-fixme[6]: Expected `BytesIO` for 2nd param but got `BinaryIO`.
    main(sys.argv[1:], sys.stdin.buffer, sys.stdout.buffer)
