#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest
from contextlib import ExitStack
from io import BufferedReader, BytesIO
from tempfile import NamedTemporaryFile, TemporaryDirectory

from .worker import Worker, HostWorker


class WorkerTest(unittest.TestCase):

    # Run the worker tool with the given input (serializing it to JSON) and
    # return the full (deserialized) output.
    # Notably, this does not actually write in chunks like proper Buck will,
    # it will not send new data based on what the worker wrote back, but the
    # tests will catch any discrepancies.
    def _run_to_completion(self, requests, worker_cls, worker_kwargs):
        out = BytesIO()
        requests = json.dumps(requests).encode()
        if not worker_kwargs:
            worker_kwargs = {}
        w = worker_cls(
            instream=BufferedReader(BytesIO(requests)),
            outstream=out,
            **worker_kwargs,
        )
        w.loop()
        return json.loads(out.getvalue().decode().strip())

    def _assert_exchange(
        self,
        requests,
        responses,
        worker_cls=HostWorker,
        worker_kwargs=None,
        *args,
        **kwargs,
    ):
        self.assertEqual(
            self._run_to_completion(requests, worker_cls, worker_kwargs),
            responses,
            *args,
            **kwargs,
        )

    def test_handshake(self):
        self._assert_exchange(
            [
                {
                    "id": 0,
                    "type": "handshake",
                    "protocol_version": "0",
                    "capabilities": [],
                }
            ],
            [
                {
                    "id": 0,
                    "type": "handshake",
                    "protocol_version": "0",
                    "capabilities": [],
                }
            ],
        )

    def test_build(self):
        with ExitStack() as stack:
            tmpdir = stack.enter_context(TemporaryDirectory())
            out_f = stack.enter_context(NamedTemporaryFile())

            args_f = stack.enter_context(NamedTemporaryFile("w"))
            json.dump(
                {
                    "tmp": tmpdir,
                    "cmd": (
                        "echo Hello stdout; echo Hello stderr >&2; "
                        f"echo Output > {out_f.name};",
                    ),
                },
                args_f,
            )
            args_f.flush()

            stdout_f = stack.enter_context(NamedTemporaryFile())
            stderr_f = stack.enter_context(NamedTemporaryFile())
            self._assert_exchange(
                [
                    {
                        "id": 0,
                        "type": "handshake",
                        "protocol_version": "0",
                        "capabilities": [],
                    },
                    {
                        "id": 1,
                        "type": "command",
                        "args_path": args_f.name,
                        "stdout_path": stdout_f.name,
                        "stderr_path": stderr_f.name,
                    },
                ],
                [
                    {
                        "id": 0,
                        "type": "handshake",
                        "protocol_version": "0",
                        "capabilities": [],
                    },
                    {
                        "id": 1,
                        "type": "result",
                        "exit_code": 0,
                    },
                ],
                worker_cls=HostWorker,
            )

            # make sure that the files were properly handled
            with open(out_f.name) as f:
                self.assertEqual(f.read(), "Output\n")
            with open(stdout_f.name) as f:
                self.assertEqual(f.read(), "Hello stdout\n")
            with open(stderr_f.name) as f:
                self.assertEqual(f.read(), "Hello stderr\n")

    def test_unknown_command(self):
        self._assert_exchange(
            [
                {
                    "id": 0,
                    "type": "handshake",
                    "protocol_version": "0",
                    "capabilities": [],
                },
                {
                    "id": 1,
                    "type": "something_unsupported",
                },
            ],
            [
                {
                    "id": 0,
                    "type": "handshake",
                    "protocol_version": "0",
                    "capabilities": [],
                },
                {"id": 1, "type": "error", "exit_code": 1},
            ],
            worker_cls=HostWorker,
        )
