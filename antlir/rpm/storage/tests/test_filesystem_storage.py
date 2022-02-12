#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import itertools
import os
import tempfile
from collections import Counter
from contextlib import contextmanager

from .storage_base_test import Storage, StorageBaseTestCase


class FilesystemStorageTestCase(StorageBaseTestCase):
    @contextmanager
    def _temp_storage(self):
        with tempfile.TemporaryDirectory() as td:
            yield Storage.make(key="test", kind="filesystem", base_dir=td)

    def test_write_and_read_back(self) -> None:
        expected_content_count = Counter()
        with self._temp_storage() as storage:
            for writes, _ in self.check_storage_impl(storage):
                # pyre-fixme[6]: For 1st param expected `Iterable[Union[memoryview,
                #  ByteString]]` but got `List[str]`.
                expected_content_count[b"".join(writes)] += 1

            # Make a histogram of the contents of the output files
            content_count = Counter()
            for f in itertools.chain.from_iterable(
                [os.path.join(p, f) for f in fs]
                for p, _, fs in os.walk(storage.base_dir)
                if fs
            ):
                with open(f, "rb") as infile:
                    content_count[infile.read()] += 1

            # Did we produce the expected number of each kind of output?
            self.assertEqual(expected_content_count, content_count)

    # This test cannot be in the base since there's no generic way to check
    # if we left a trace on the storage system -- there's no ID to fetch.
    def test_uncommitted(self):
        with self._temp_storage() as storage:
            self.assertEqual([], os.listdir(storage.base_dir))
            with storage.writer() as writer:
                writer.write(b"foo")
            self.assertEqual([], os.listdir(storage.base_dir))
            with self.assertRaisesRegex(RuntimeError, "^abracadabra$"):
                with storage.writer() as writer:
                    raise RuntimeError("abracadabra")
            self.assertEqual([], os.listdir(storage.base_dir))
