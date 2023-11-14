#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The way that TPX invokes static list binaries is very unconducive to supporting
it within antlir2.

Since this requires some really hacky interpretation of argv, this is a simple
python script that branches on the test type and then parses args and forwards
them to the underlying static listing implementation.

The args as received by this script are a terrible mix of the args that
antlir2's image_test produces for internal use, the args for the static lister
and other args that the static lister in TPX thinks are necessary for vanilla
tests of this type.
"""

import argparse
import os
import subprocess
import sys


def main():
    sys.argv.pop(0)
    which = sys.argv.pop(0)
    if which == "cpp":
        parser = argparse.ArgumentParser()
        parser.add_argument("image_test_bin")
        parser.add_argument("--wrap", required=True)
        parser.add_argument("--spec", required=True)
        parser.add_argument("test_type", choices=["gtest"])
        parser.add_argument("cmd", nargs="+")
        args = parser.parse_args(sys.argv)
        os.execv(args.wrap, [args.wrap] + args.cmd)
    if which == "py":
        parser = argparse.ArgumentParser()
        parser.add_argument("--wrap", required=True)
        parser.add_argument("--spec", required=True)
        parser.add_argument("--json-output", required=True)
        parser.add_argument("image_test_bin")
        parser.add_argument("test_type", choices=["pyunit"])
        parser.add_argument("cmd", nargs="+")
        args = parser.parse_args(sys.argv)
        os.execv(args.wrap, [args.wrap, "--json-output", args.json_output] + args.cmd)

    raise Exception(f"Unknown static list wrapper: {which}")


if __name__ == "__main__":
    main()
