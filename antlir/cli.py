#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Common CLI boilerplate for Antlir binaries."
import argparse
import json
import os
import sys
from contextlib import contextmanager
from typing import AnyStr, Iterator, Mapping

from antlir.common import init_logging
from antlir.fs_utils import Path

# python < 3.9 doesn't have `removesuffix`
def _removesuffix(s, suffix):
    if s.endswith(suffix):
        return s[:-len(suffix)]
    else:
        return s

# pyre-fixme[34]: `Variable[AnyStr <: [str, bytes]]` isn't present in the
#  function's parameters.
def _load_targets_and_outputs(arg: str) -> Mapping[AnyStr, Path]:
    # The targets names we will be looking up are the names of
    # the configured_aliases, not the backing `-actual` target.
    return {
        _removesuffix(k, "-actual"): v for k,v in json.loads(Path(arg).read_text()).items()
    }


def add_targets_and_outputs_arg(parser: argparse.ArgumentParser):
    parser.add_argument(
        "--targets-and-outputs",
        type=_load_targets_and_outputs,
        help="Load and parse a json document containing a mapping"
        "of targets -> on disk outputs",
    )


def add_antlir_debug_arg(parser: argparse.ArgumentParser):
    parser.add_argument(
        "--debug",
        action="store_true",
        default=bool(os.environ.get("ANTLIR_DEBUG")),
        help="Log more -- also enabled via the ANTLIR_DEBUG env var",
    )


# pyre-fixme[13]: Attribute `args` is never initialized.
# pyre-fixme[13]: Attribute `parser` is never initialized.
class CLI:
    parser: argparse.ArgumentParser
    args: argparse.Namespace


# Future: This should get some tests if it gets any more elaborate.
@contextmanager
def init_cli(description: str) -> Iterator[CLI]:
    cli = CLI()
    cli.parser = argparse.ArgumentParser(
        description=description,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    yield cli
    add_antlir_debug_arg(cli.parser)
    cli.args = Path.parse_args(cli.parser, sys.argv[1:])
    init_logging(debug=cli.args.debug)
