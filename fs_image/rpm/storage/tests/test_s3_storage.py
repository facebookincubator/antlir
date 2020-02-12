#!/usr/bin/env python3
import logging
import os
import tempfile
import unittest.mock

from functools import wraps

from fs_image.common import load_location
from .storage_base_test import Storage
from .cli_object_storage_base_test import CLIObjectStorageBaseTestCase
from ..s3_storage import S3Storage


def mock_s3_cli(fn):
    @wraps(fn)
    def mock(*args, **kwargs):
        with tempfile.TemporaryDirectory() as td:
            # We mock `_path_for_storage_id` such that the base dir
            # is always going to be the TempDir we created
            def _mock_path_for_storage_id(sid):
                return os.path.join(td, sid)

            # Instead of calls to `aws s3`, we want to call
            # `mock-s3-cli instead`
            mock_s3_cli_path = load_location(
                'rpm.storage.tests', 'mock-s3-cli'
            )
            with unittest.mock.patch.object(
                S3Storage, '_base_cmd',
                return_value=[
                    mock_s3_cli_path
                ],
            ), unittest.mock.patch.object(
                S3Storage, '_path_for_storage_id',
                side_effect=_mock_path_for_storage_id,
            ):
                return fn(*args, **kwargs)
    return mock


class S3StorageTestCase(CLIObjectStorageBaseTestCase):

    def setUp(self):
        logging.getLogger().setLevel(logging.DEBUG)
        self.storage = Storage.make(
            key='test', kind='s3', timeout_seconds=3,
        )

    def _decorate_id(self, sid: str) -> str:
        # In the case of S3, we are using our `mock-s3-cli`
        # which is automatically configured to write to a tmp
        # directory; therefore, there is no need to decorate
        # `sid`s to force the storage to use a test directory
        return sid

    @mock_s3_cli
    def test_write_and_read_back(self):
        self._test_write_and_read_back(S3Storage)

    @mock_s3_cli
    def test_uncommitted(self):
        proc = self._test_uncommited(S3Storage)
        self.assertFalse(proc.stdout)

    @mock_s3_cli
    def test_error_cleanup(self):
        # Re-patch the _path_for_storage_id method to always
        # return a path that doesn't exist.
        def _mock_path_for_storage_id(sid):
            return os.path.join(
                '/not/a/valid/path',
                sid,
            )
        with unittest.mock.patch.object(
            S3Storage, '_path_for_storage_id',
            side_effect=_mock_path_for_storage_id,
        ):
            self._test_error_cleanup('s3')

    def test_base_cmd(self):
        self.assertEqual(
            Storage.make(
                key='test', kind='s3', timeout_seconds=123
            )._base_cmd('--cat', 'meow'),
            [
                'aws', 's3',
                '--cli-read-timeout', '123',
                '--cli-connect-timeout', '123',
                '--cat', 'meow',
            ],
        )

    def test_path_for_storage_id(self):
        s3_bucket_name = 's3://cats-go-meow'
        blob_sid = '123'
        with unittest.mock.patch.object(
            S3Storage, 'AWS_S3_BUCKET',
            s3_bucket_name,
        ):
            self.assertEqual(
                Storage.make(
                    key='test', kind='s3',
                )._path_for_storage_id(blob_sid),
                os.path.join(
                    s3_bucket_name, 'flat', blob_sid,
                )
            )
