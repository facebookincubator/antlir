#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
from contextlib import contextmanager
from unittest.mock import ANY, MagicMock, patch

from .. import s3_storage
from .storage_base_test import Storage, StorageBaseTestCase


class S3StorageTestCase(StorageBaseTestCase):
    bucket = "antlir-test"
    region = "test-region"
    prefix = "test/prefix"

    # some methods to mock an in-memory s3 bucket
    # this matches the read-after-write consistency guarantee from the real s3
    def _mock_upload_fileobj(self, contents, bucket: str, key: str):
        assert bucket == self.bucket
        self.objects[key] = contents.getbuffer()

    def _mock_delete_object(self, bucket: str, key: str):
        assert bucket == self.bucket
        del self.objects[key]

    @contextmanager
    def _mock_open_url(self, url: str):
        urlprefix = f"https://{self.bucket}.s3-{self.region}.amazonaws.com/"
        assert url.startswith(urlprefix)
        key = url[len(urlprefix) :]
        yield io.BytesIO(self.objects[key])

    def setUp(self):
        self.objects = {}
        self.s3 = MagicMock()
        self.s3.upload_fileobj.side_effect = self._mock_upload_fileobj
        self.s3.delete_object.side_effect = self._mock_delete_object
        self.storage = Storage.make(
            key="test",
            kind="s3",
            bucket=self.bucket,
            prefix=self.prefix,
            region=self.region,
            timeout_seconds=3,
        )
        self.boto3_client_patch = patch("boto3.client")
        boto3_client = self.boto3_client_patch.start()
        boto3_client.return_value = self.s3
        self.assertEqual(self.storage.s3, self.s3)
        s3_storage.open_url = self._mock_open_url

    def tearDown(self):
        self.boto3_client_patch.stop()

    def test_write_and_read_back(self):
        # Do a bunch of mock writes and ensure that the s3 client was called
        # with each storage id s3 key.
        # The s3 client does not do any partial writes or buffering, so as long
        # as it is called with the correct contents, that is enough to pass.
        for contents, sid in self.check_storage_impl(self.storage):
            self.s3.upload_fileobj.assert_any_call(
                ANY, "antlir-test", self.storage._object_key(sid)
            )
            # ensure that the written data was sent to s3 correctly, as one
            # blob regardless of how many chunks it was input as
            with self.storage.reader(sid) as f:
                actual = f.read()
                self.assertEqual(actual, b"".join(contents))

    def test_delete(self):
        with self.storage.writer() as out:
            out.write(b"Hello world!")
            sid = out.commit()
        key = self.storage._object_key(sid)
        self.assertIn(key, self.objects)
        self.storage.remove(sid)
        self.assertNotIn(key, self.objects)
        self.s3.delete_object.called_with(self.bucket, key)

    def test_reader_url(self):
        with patch("antlir.rpm.storage.s3_storage.open_url") as open_url:
            with self.storage.reader("1234") as _:
                pass
            open_url.assert_called_with(
                "https://antlir-test.s3-test-region.amazonaws.com/"
                "test/prefix/1234"
            )

    def test_object_key(self):
        blob_sid = "123"
        self.assertEqual(
            self.storage._object_key(blob_sid), f"test/prefix/{blob_sid}"
        )
