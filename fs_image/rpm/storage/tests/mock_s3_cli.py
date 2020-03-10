#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Simple Python binary which implements the aws s3 cli interface.
Used to mock `aws s3` commands used in `tests/s3_storage.py`.
'''
import argparse
import os
import sys

from fs_image.common import nullcontext


def cp(args):
    # The aws s3 cli uses "-" to represent stdin/stdout
    with (
        nullcontext(sys.stdin.buffer) if args.src == '-'
        else open(
            args.src, 'rb',
        )
    ) as from_file, (
        nullcontext(sys.stdout.buffer) if args.dest == '-'
        else open(
            args.dest, 'wb',
        )
    ) as to_file:
        to_file.write(from_file.read())


def rm(args):
    os.remove(args.path)


def ls(args):
    # We are not implementing the full `ls` API.
    # We are simply stubbing out the return behaviour which is
    # sufficient for tests.
    if not os.path.exists(args.path):
        raise FileExistsError


def main(argv):
    # Ensure that AWS keys are passed through environment variables
    # Raise a KeyError if either of the keys are not provided
    os.environ['AWS_SECRET_ACCESS_KEY']
    os.environ['AWS_ACCESS_KEY_ID']

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    subparsers = parser.add_subparsers()

    parser_cp = subparsers.add_parser('cp')
    parser_cp.add_argument('src', help='string of src path, use "-" for stdin.')
    parser_cp.add_argument(
        'dest', help='string of dest path, use "-" for stdout.'
    )
    parser_cp.set_defaults(func=cp)

    parser_rm = subparsers.add_parser('rm')
    parser_rm.add_argument('path', help='path (string)')
    parser_rm.set_defaults(func=rm)

    parser_ls = subparsers.add_parser('ls')
    parser_ls.add_argument('path', help='path (string)')
    parser_ls.set_defaults(func=ls)

    args = parser.parse_args(argv)
    args.func(args)


if __name__ == '__main__':  # pragma: no cover
    main(sys.argv[1:])
