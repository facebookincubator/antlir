#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import logging
import uuid
from contextlib import contextmanager
from typing import ContextManager

import boto3
from antlir.common import get_logger
from antlir.rpm.storage.storage import _CommitCallback

from ..open_url import open_url
from ..storage import Storage, StorageInput, StorageOutput


log = get_logger()

boto3.set_stream_logger("", logging.WARNING)


class S3Storage(Storage, plugin_kind="s3"):
    def __init__(
        self,
        *,
        key: str,
        bucket: str,
        prefix: str,
        region: str,
        timeout_seconds: float = 400,
    ):
        self.key = key
        self.bucket = bucket
        self.prefix = prefix
        self.region = region
        self.timeout_seconds = timeout_seconds
        # Reads require no credentials, writes go through AWS authentication
        # and will fail if the required environment variables are not set
        # Full details here, but in practice it's ok to ignore the intricacies.
        # https://boto3.amazonaws.com/v1/documentation/api/latest/guide/credentials.html
        self.s3 = boto3.client("s3")

    @classmethod
    def _make_storage_id(cls) -> str:
        return str(uuid.uuid4()).replace("-", "")

    def _object_key(self, sid: str) -> str:
        key = sid
        if ":" in key:
            key = self.strip_key(sid)
        return f"{self.prefix}/{key}"

    @contextmanager
    def reader(self, sid: str) -> ContextManager[StorageInput]:
        url = (
            f"https://{self.bucket}.s3-{self.region}.amazonaws.com/"
            + self._object_key(sid)
        )
        with open_url(url) as f:
            yield StorageInput(input=f)

    @contextmanager
    def writer(self) -> ContextManager[StorageOutput]:
        sid = self._make_storage_id()
        key = self._object_key(sid)
        log_prefix = f"{self.__class__.__name__}"
        log.debug(f"{log_prefix} - Writing to {key}")

        buf = io.BytesIO()

        @contextmanager
        def get_id_and_release_resources():
            buf.seek(0)
            self.s3.upload_fileobj(buf, self.bucket, key)
            # S3 does not do partial puts, if the request returned 200, then
            # there is read-after-write guarantees
            yield sid

        with _CommitCallback(self, get_id_and_release_resources) as commit:
            yield StorageOutput(output=buf, commit_callback=commit)

    def remove(self, sid: str) -> None:
        key = self._object_key(sid)
        self.s3.delete_object(self.bucket, key)
