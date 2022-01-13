#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Usage:

    python3 -m btrfs_diff.tests.pring_gold_demo_sendstreams stream_name

Prints to stdout the binary send-stream corresponding to one of the
scripts defined in `demo_sendstreams.py`.
"""
import os
import pickle
import sys


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 1

    with open(
        os.path.join(os.path.dirname(__file__), "gold_demo_sendstreams.pickle"),
        "rb",
    ) as infile:
        sys.stdout.buffer.write(pickle.load(infile)[argv[1]]["sendstream"])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
