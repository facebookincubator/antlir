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
antlir2_vm produces for internal use, the args for the static lister and other
args that the static lister in TPX thinks are necessary for vanilla tests of
this type.
"""

import argparse
import os
import sys


def main():
    sys.argv.pop(0)
    which = sys.argv[0]
    if which == "cpp":
        print(sys.argv)
        # First get the real gtest lister binary
        parser = argparse.ArgumentParser()
        parser.add_argument("--wrap", required=True)
        args, extras = parser.parse_known_args(sys.argv)
        wrapped = args.wrap

        # Then use another ArgumentParser to call that wrapped binary
        parser = argparse.ArgumentParser()
        parser.add_argument("binary")
        args, extras = parser.parse_known_args(extras[-2:])
        os.execv(wrapped, [wrapped, args.binary] + extras)
    if which == "py":
        parser = argparse.ArgumentParser()
        parser.add_argument("--wrap", required=True)
        parser.add_argument("--json-output", required=True)
        args, extra = parser.parse_known_args(sys.argv)

        idx = extra.index("pyunit")
        inner_test = extra[idx + 1]
        os.execv(args.wrap, [args.wrap, "--json-output", args.json_output, inner_test])

    raise Exception(f"Unknown static list wrapper: {which}")


if __name__ == "__main__":
    main()
