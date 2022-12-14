#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse

from antlir import btrfsutil


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("subvolume")
    args = parser.parse_args()
    btrfsutil.delete_subvolume(args.subvolume, recursive=True)
    return 0


if __name__ == "__main__":
    main()
