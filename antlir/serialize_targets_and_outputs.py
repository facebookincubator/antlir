#!/usr/bin/python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import sys


def parse_and_dump(stdin, stdout, delim):
    """
    This reads a list that contains a delimited list of buck target ->
    on disk location mappings.  Since this binary exists entirely
    to work with the output that buck provides via the
    `query_targets_and_outputs` macro (described here:
    https://buck.build/function/string_parameter_macros.html),
    this requires the delimiter to be provided.  This ensures that the delimiter
    is defined in the companion `targets_and_outputs_arg_list` helper located
    in `//antlir/bzl:target_helpers.bzl`
    """
    targs_locs = stdin.read().rstrip().split(delim)
    data = dict(
        zip(
            targs_locs[::2],
            targs_locs[1::2],
        )
    )

    stdout.write(json.dumps(data))


if __name__ == "__main__":
    # First argument passed to the binary should be the separator
    # that is used for the target/location mapping
    parse_and_dump(sys.stdin, sys.stdout, sys.argv[1])  # pragma: no cover
