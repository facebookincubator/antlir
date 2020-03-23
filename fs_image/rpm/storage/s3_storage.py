#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import logging

from contextlib import contextmanager
from typing import ContextManager, List

from rpm.open_url import open_url
from .cli_object_storage import CLIObjectStorage
from ..storage import StorageInput

log = logging.getLogger(__file__)


class S3Storage(CLIObjectStorage, plugin_kind='s3'):

    def __init__(self,
            *,
            key: str,
            bucket: str,
            prefix: str,
            read_scheme: str = 'https',
            region: str,
            timeout_seconds: float=400
        ):
        
        self.key = key
        self.bucket = bucket
        self.prefix = prefix
        self.region = region
        self.timeout_seconds = timeout_seconds
        self.read_scheme = read_scheme

    def _path_for_storage_id(self, sid: str) -> str:
        return os.path.join(f's3://{self.bucket}', self.prefix, sid)

    def _base_cmd(self, *args) -> List[str]:
        return [
            'aws', 's3',
            '--cli-read-timeout', str(self.timeout_seconds),
            '--cli-connect-timeout', str(self.timeout_seconds),
            *args,
        ]


    def _read_cmd(self, *args, path: str) -> List[str]:
        raise NotImplementedError

    def _write_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ['cp', '-', path]

    def _remove_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ['rm', path]

    def _exists_cmd(self, *args, path: str) -> List[str]:
        return self._base_cmd(*args) + ['ls', path]

    def _configured_env(self):
        env = os.environ.copy()
        if 'AWS_SECRET_ACCESS_KEY' not in env:
            raise RuntimeError('AWS_SECRET_ACCESS_KEY is required for writing to S3 storage')

        if 'AWS_ACCESS_KEY_ID' not in env:
            raise RuntimeError('AWS_ACCESS_KEY_ID is required for writing to S3 storage')

        return env

    @contextmanager
    def reader(self, sid: str) -> ContextManager[StorageInput]:
        url = f'{self.read_scheme}://{self.bucket}.s3-{self.region}.amazonaws.com/{self.prefix}/{self.strip_key(sid)}'
        log.debug(f'url: {url}')
        with open_url(url) as f:
            yield StorageInput(input=f)
