#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import json
import subprocess
import sys
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, Mapping

from antlir.common import get_logger
from antlir.fs_utils import Path


logger = get_logger()


@dataclass
class Worker(ABC):
    instream: io.BufferedReader
    outstream: io.BytesIO
    _buf: bytes = b""

    def loop(self):
        # read the handshake open first
        assert self.instream.read(1) == b"["
        self.outstream.write(b"[")
        while True:
            self._buf += self.instream.read1(1024)
            logger.debug(
                f"Looking for a well-formed JSON request in {self._buf}"
            )
            if self._buf == b"]":
                break
            if self._buf and chr(self._buf[0]) == ",":
                self._buf = self._buf[1:]
            for i in range(len(self._buf)):
                try:
                    req = json.loads(self._buf[: i + 1])
                    self._buf = self._buf[i + 1 :]
                    resp = self.handle(req)
                    resp = json.dumps(resp).encode()
                    if req["type"] != "handshake":
                        self.outstream.write(b",")
                    self.outstream.write(resp)
                    self.outstream.flush()
                    logger.debug(f"request: {req}")
                    logger.debug(f"response: {resp}")
                    break
                except json.JSONDecodeError:
                    continue
        logger.info("Buck closed the stream, exiting...")
        self.outstream.write(b"]")

    def handle(self, req: Mapping[str, Any]) -> Mapping[str, Any]:
        if req["type"] == "handshake":
            return {
                "id": req["id"],
                "type": "handshake",
                "protocol_version": "0",
                "capabilities": [],
            }
        if req["type"] == "command":
            with open(req["args_path"], "r") as f:
                args = json.load(f)
            returncode = self.handle_command(
                args, Path(req["stdout_path"]), Path(req["stderr_path"])
            )
            return {"id": req["id"], "type": "result", "exit_code": returncode}
        # unknown command returns error
        return {
            "id": req["id"],
            "type": "error",
            "exit_code": 1,
        }

    @abstractmethod
    def handle_command(self, args: str, stdout: Path, stderr: Path) -> int:
        pass  # pragma: no cover


class HostWorker(Worker):
    def handle_command(self, args: str, stdout: Path, stderr: Path) -> int:
        with stdout.open("wb") as stdout, stderr.open("wb") as stderr:
            res = subprocess.run(
                args["cmd"], shell=True, stdout=stdout, stderr=stderr
            )
        return res.returncode


if __name__ == "__main__":  # pragma: no cover
    w = HostWorker(sys.stdin.buffer, sys.stdout.buffer)
    w.loop()
