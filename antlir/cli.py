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


def _load_targets_and_outputs(arg: AnyStr) -> Mapping[AnyStr, Path]:
    return json.loads(Path(arg).read_text())


def add_targets_and_outputs_arg(
    parser: argparse.ArgumentParser, *, action=None, suppress_help: bool = False
):
    parser.add_argument(
        "--targets-and-outputs",
        type=_load_targets_and_outputs,
        help=argparse.SUPPRESS
        if suppress_help
        else "Load and parse a json document containing a mapping"
        "of targets -> on disk outputs",
        action=action,
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
