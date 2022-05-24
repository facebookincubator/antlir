#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import logging
import os.path
import uuid
import warnings
from contextlib import contextmanager
from typing import ContextManager

from antlir.common import get_logger
from antlir.rpm.storage.storage import _CommitCallback

from ..open_url import open_url
from ..storage import Storage, StorageInput, StorageOutput


# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()


class S3Storage(Storage, plugin_kind="s3"):
    def __init__(
        self,
        *,
        key: str,
        bucket: str,
        prefix: str,
        region: str,
        timeout_seconds: float = 400,
    ) -> None:
        self.key = key
        self.bucket = bucket
        self.prefix = prefix
        self.region = region
        self.timeout_seconds = timeout_seconds

    def _object_key(self, sid: str) -> str:
        key = sid
        if ":" in key:
            key = self.strip_key(sid)
        return key

    @contextmanager
    def reader(self, sid: str) -> ContextManager[StorageInput]:
        key = self._object_key(sid)
        url = f"https://{self.bucket}.s3-{self.region}.amazonaws.com/{key}"
        with open_url(url) as f:
            # pyre-fixme[7]: Expected
            #  `ContextManager[antlir.rpm.storage.storage.StorageInput]` but got
            #  `Generator[antlir.rpm.storage.storage.StorageInput, None, None]`.
            yield StorageInput(input=f)

    @property
    # pyre-fixme[3]: Return type must be annotated.
    def s3(self):
        # botocore does not support running from a pex/zip (aka a 'standalone'
        # binary that gets installed in an image), so only import it when on
        # the write path, which happens in an 'inplace' context on the host
        import boto3

        boto3.set_stream_logger("", logging.WARNING)
        # botocore is not threadsafe, each thread needs its own session.
        session = boto3.session.Session()
        # Reads require no credentials, writes go through AWS authentication
        # and will fail if the required environment variables are not set
        # Full details here, but in practice it's ok to ignore the intricacies.
        # https://boto3.amazonaws.com/v1/documentation/api/latest/guide/credentials.html
        s3 = session.resource("s3")
        return s3.Bucket(self.bucket)

    @contextmanager
    def writer(self) -> ContextManager[StorageOutput]:
        sid = str(uuid.uuid4()).replace("-", "")
        key = os.path.join(self.prefix, sid)
        log_prefix = f"{self.__class__.__name__}"
        log.debug(f"{log_prefix} - Writing to {key}")

        buf = io.BytesIO()

        @contextmanager
        # pyre-fixme[53]: Captured variable `buf` is not annotated.
        # pyre-fixme[53]: Captured variable `key` is not annotated.
        # pyre-fixme[3]: Return type must be annotated.
        def get_id_and_release_resources():
            buf.seek(0)
            # boto3 spews unclosed resource warnings everywhere, without a
            # possible fix client side :(
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ResourceWarning)
                self.s3.upload_fileobj(buf, key)
            # S3 does not do partial puts, if the request returned 200, then
            # there is read-after-write guarantees
            yield key

        # pyre-fixme[6]: Expected `ContextManager[typing.Any]` for 2nd param
        # but got `() -> Any`.
        with _CommitCallback(self, get_id_and_release_resources) as commit:
            # pyre-fixme[7]: Expected
            # `ContextManager[antlir.rpm.storage.storage.StorageOutput]` but
            # got `Generator[antlir.rpm.storage.storage.StorageOutput, None,
            # None]`.
            yield StorageOutput(output=buf, commit_callback=commit)

    def remove(self, sid: str) -> None:
        key = self._object_key(sid)
        self.s3.delete_objects(Delete={"Objects": [{"Key": key}]})
