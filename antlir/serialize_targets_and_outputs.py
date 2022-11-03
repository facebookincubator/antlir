#!/usr/bin/python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import configparser
import json
import sys
from typing import Dict, Generator, Mapping, Sequence

from antlir.artifacts_dir import find_buck_cell_root
from antlir.fs_utils import Path


class BuckConfigParser(configparser.ConfigParser):
    def __init__(self) -> None:
        # strict=False allows sections to be repeated in the .buckconfig, among
        # other things, and Buck allows this more relaxed syntax in configs.
        super().__init__(strict=False)

    def optionxform(self, optionstr: str) -> str:
        # Prevent keys from being converted to lowercase.
        return optionstr


def find_cell_root(targets_to_outputs: Mapping[str, str]) -> Path:
    return find_buck_cell_root(Path(next(iter(targets_to_outputs.values()))))


def get_main_cell_names(
    buck_config: BuckConfigParser,
) -> Generator[str, None, None]:
    yield from (
        cell for cell, path in buck_config["repositories"].items() if path == "."
    )


def make_target_path_map(targets_locations: Sequence[str]) -> Dict[str, str]:
    """
    Transform a flattened sequence of target and output location pairs,
    following the pattern [<targetA>, <outputA>, <targetB>, <outputB>, ...],
    into a mapping from target name to output path.

    Multiple entries will be created for targets in the main cell, with and
    without the full cell name. This ensures that targets can be found with
    any fully-qualified target name that refences them. For example:

    If the main cell is A, and target //foo:bar maps to /foo/bar then the
    following entries will be generated in the mapping:
    {
        "//foo:bar": "/foo/bar",
        "A//foo:bar": "/foo/bar",
    }
    The same is true for the reverse. Given A//foo:bar, the same entries would
    be generated. Targets not in the main cell are not given multiple entries.
    """

    it = iter(targets_locations)
    targets_to_outputs = dict(zip(it, it))
    if targets_to_outputs:
        cell_root = find_cell_root(targets_to_outputs)
        buck_config = BuckConfigParser()
        buck_config.read(cell_root / ".buckconfig")
        for cell in get_main_cell_names(buck_config):
            for target, output in targets_to_outputs.copy().items():
                if target.startswith("//"):
                    targets_to_outputs[cell + target] = output
                if target.startswith(cell + "//"):
                    targets_to_outputs[target[len(cell) :]] = output
    return targets_to_outputs


def main(stdin, stdout, delim) -> None:
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
    stdout.write(json.dumps(make_target_path_map(targs_locs)))


if __name__ == "__main__":
    # First argument passed to the binary should be the separator
    # that is used for the target/location mapping
    main(sys.stdin, sys.stdout, sys.argv[1])  # pragma: no cover
