#!/usr/bin/env python3
import logging
import unittest.mock

from .storage_base_test import Storage
from .cli_object_storage_base_test import CLIObjectStorageBaseTestCase
from ..s3_storage import S3Storage


class S3StorageTestCase(CLIObjectStorageBaseTestCase):

    def setUp(self):
        logging.getLogger().setLevel(logging.DEBUG)
        self.storage = Storage.make(
            key='test', kind='s3', timeout_seconds=3,
        )

    def _decorate_id(self, sid: str) -> str:
        # Prefix all IDs with 'unittest/' so that, if needed, we can
        # manually clean the leaked blobs
        return 'unittest/' + sid

    def test_write_and_read_back(self):
        self._test_write_and_read_back(S3Storage)

    def test_uncommitted(self):
        proc = self._test_uncommited(S3Storage)
        self.assertFalse(proc.stdout)

    def test_error_cleanup(self):
        with unittest.mock.patch.object(
            S3Storage, 'AWS_S3_BUCKET',
            's3://not-a-valid-s3-bucket',
        ):
            self._test_error_cleanup('s3')
