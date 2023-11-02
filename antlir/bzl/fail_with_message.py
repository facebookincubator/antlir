# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse

from antlir.common import get_logger, init_logging

log = get_logger()


# This is covered by `test_fail_with_message_e2e` in `test-fail-with-message`.
def _parse_cmdline_args() -> argparse.Namespace:  # pragma: no cover
    from antlir.cli import init_cli

    with init_cli("Logs the error message as provided by argument.") as cli:
        cli.parser.add_argument(
            "--message",
            help=argparse.SUPPRESS,
            type=str,
            required=True,
        )
    return cli.args


def log_failure_message(msg: str) -> None:
    log.error(msg)


def main() -> None:  # pragma: no cover
    args = _parse_cmdline_args()
    init_logging()
    log_failure_message(args.message)


# This is covered by `test_fail_with_message_e2e` in `test-fail-with-message`.
if __name__ == "__main__":
    main()  # pragma: no cover
