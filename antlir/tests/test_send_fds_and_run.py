#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import os
import re
import subprocess
import tempfile
import unittest

from antlir.common import pipe
from antlir.send_fds_and_run import parse_opts, send_fds_and_popen


def _run(argv):
    env = os.environ
    env.pop("SUDO_COMMAND", None)  # So we can assert whether `sudo` was used
    with send_fds_and_popen(
        parse_opts(argv),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as proc:
        stdout, stderr = proc.communicate()
    return proc.returncode, stdout.decode(), stderr.decode()


def _clean_err(err):
    logging.info(f"From wrapper:\n{err}")
    err = [
        l
        for l in err.split("\n")
        if not re.search(r"D.+recv_fds_and_run.py:", l)
    ]
    assert err[-1] == ""  # `split` will always leave us at least one empty str
    return err[:-1]


class SendFdsAndRunTestCase(unittest.TestCase):
    def test_send_fds(self) -> None:
        for sudo_args in [[], ["--sudo"], ["--sudo", "--sudo-arg", "moo=cow"]]:
            # Check basic wrapper execution, except for FD passing (we
            # pass 0 FDs), and `--no-set-listen-fds`.
            ret, out, err = _run(
                [
                    *sudo_args,
                    "--",
                    "bash",
                    "-c",
                    "echo $SUDO_COMMAND ; echo $$ ; "
                    "printenv LISTEN_FDS LISTEN_PID ; exit 37",
                ]
            )
            self.assertEqual([], _clean_err(err))
            self.assertEqual(37, ret)
            sudo, sh_pid, listen_fds, listen_pid = out.rstrip("\n").split("\n")
            self.assertEqual("0", listen_fds)
            self.assertEqual(sh_pid, listen_pid)
            self.assertEqual(bool(sudo), bool(sudo_args), f"{out} {sudo_args}")

            # Exercise `--no-set-listen-fds` and exit code 0.
            ret, out, err = _run(
                [
                    *sudo_args,
                    "--no-set-listen-fds",
                    "--",
                    "bash",
                    "-c",
                    'echo "$LISTEN_FDS/$LISTEN_PID"',
                ]
            )
            self.assertEqual((0, "/\n", []), (ret, out, _clean_err(err)))

            # Exercise actual FD passing, both input and output
            with pipe() as (r_fd4, w_fd4), pipe() as (
                r_fd3,
                w_fd3,
            ), subprocess.Popen(
                ["echo", "hi-diddly-ho"], stdout=w_fd4
            ), tempfile.TemporaryFile() as tf:
                w_fd4.close()  # or the Flanders cat might wait forever
                with subprocess.Popen(
                    ["cat"], stdin=r_fd3, stdout=tf.fileno()
                ) as homer_proc:
                    try:
                        ret, out, err = _run(
                            [
                                *sudo_args,
                                f"--fd={w_fd3.fileno()}",
                                f"--fd={r_fd4.fileno()}",
                                "--",
                                "bash",
                                "-c",
                                'echo "$LISTEN_FDS" ; cat <&4 ; echo doh >&3',
                            ]
                        )
                    finally:
                        w_fd3.close()  # or the Homer cat might wait forever
                self.assertEqual([], _clean_err(err))
                self.assertEqual("2\nhi-diddly-ho\n", out)
                self.assertEqual(0, ret)
                tf.seek(0)
                self.assertEqual(b"doh\n", tf.read())
                homer_proc.wait()
