#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
from typing import List

from .cli_object_storage import CLIObjectStorage


class S3Storage(CLIObjectStorage, plugin_kind="s3"):

    AWS_S3_BUCKET = "s3://AWS_S3_BUCKET"
    AWS_ACCESS_KEY_ID = "AWS_ACCESS_KEY_ID"
    AWS_SECRET_ACCESS_KEY = "AWS_SECRET_ACCESS_KEY"

    def __init__(self, *, key: str, timeout_seconds: float = 400):
        self.key = key
        self.timeout_seconds = timeout_seconds

    def _path_for_storage_id(self, sid: str) -> str:
        return os.path.join(self.AWS_S3_BUCKET, "flat", sid)

    def _base_cmd(self, *args) -> List[str]:
        return [
            "aws",
            "s3",
            "--cli-read-timeout",
            str(self.timeout_seconds),
            "--cli-connect-timeout",
            str(self.timeout_seconds),
            *args,
        ]

    def _read_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ["cp", path, "-"]

    def _write_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ["cp", "-", path]

    def _remove_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ["rm", path]

    def _exists_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ["ls", path]

    def _configured_env(self):
        # Configure env with AWS credentials
        env = os.environ.copy()
        env["AWS_ACCESS_KEY_ID"] = self.AWS_ACCESS_KEY_ID
        env["AWS_SECRET_ACCESS_KEY"] = self.AWS_SECRET_ACCESS_KEY
        return env
