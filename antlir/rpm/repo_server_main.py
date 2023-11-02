#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This wraps the actual main in `repo_server.py` to make sure that we
handle SIGTERM as early as possible. This is our "graceful termination"
signal -- and not `SIGINT` -- because everything in the Python stack
interprets `SIGINT` as "dump a stack trace". If we run a fast program like:

    buck run :foo=container -- --user=root -- echo hi

Then, we often end up shutting down the repo server before it finished
starting.  By sending `SIGTERM` we avoid dumping a stack trace in this case.

Once the server is running, we want to translate `SIGTERM` to
`KeyboardInterrupt` to permit FB-internal storage implementations to clean
up their allocated resources.
"""
import signal


def _sig_raise_keyboard_interrupt(signum, stackframe):
    raise KeyboardInterrupt


def invoke_main() -> None:
    try:
        signal.signal(signal.SIGTERM, _sig_raise_keyboard_interrupt)

        from antlir.rpm.repo_server import main

        main()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    invoke_main()
