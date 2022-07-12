#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Usage:

    alias demo_sendstream='python3 -m btrfs_diff.tests.gold_demo_sendstreams'
    demo_sendstream create_ops | python3 -m btrfs_diff.examples.dump_sendstream

Reads a send-stream from stdin, prints the Python parse to stdout. This
output is only meant for human consumption -- but it would be easy to
instead serialize each item to something parseable like JSON.

Besides providing a code example, the main advantage of this program
compared to `btrfs receive --dump` is that our parsing & output has no known
bugs, and is backed by through test coverage.  Read the heading of
`parse_dump.py` for the known bugs in `--dump`.
"""
import sys

from antlir.btrfs_diff.parse_send_stream import parse_send_stream


def main(argv):
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 1

    for item in parse_send_stream(sys.stdin.buffer):
        print(item)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
