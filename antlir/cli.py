#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Common CLI boilerplate for Antlir binaries."
import argparse
import os
import sys
from contextlib import contextmanager
from typing import Iterable, Iterator, Optional

from antlir.common import init_logging
from antlir.fs_utils import MehStr, Path


def add_antlir_debug_arg(parser: argparse.ArgumentParser) -> None:
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
def init_cli(
    description: str, argv: Optional[Iterable[MehStr]] = None
) -> Iterator[CLI]:
    cli = CLI()
    cli.parser = argparse.ArgumentParser(
        description=description,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    yield cli
    add_antlir_debug_arg(cli.parser)
    cli.args = Path.parse_args(cli.parser, argv if argv is not None else sys.argv[1:])
    init_logging(debug=cli.args.debug)
