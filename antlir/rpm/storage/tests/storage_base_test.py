#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import List, Tuple
from unittest.mock import MagicMock, patch

from antlir.common import get_logger

from antlir.rpm.storage import Storage  # Module import to ensure we get plugins

log = get_logger()


class StorageBaseTestCase(unittest.TestCase):
    "A tiny test suite that can be used to check any Storage implementation."

    def _check_write_and_read(self, storage: Storage, writes: List[bytes]):
        log.info(f"_check_write_and_read: {writes}")
        # pyre-fixme[16]: `Storage` has no attribute `writer`.
        with storage.writer() as output:
            for piece in writes:
                output.write(piece)
            sid = output.commit()
        # pyre-fixme[16]: `Storage` has no attribute `reader`.
        with storage.reader(sid) as input:
            written = b"".join(writes)
            partial_read = input.read(3)
            if written:
                self.assertGreater(len(partial_read), 0)
            self.assertLessEqual(len(partial_read), 3)
            self.assertEqual(written, partial_read + input.read())
        return sid

    def check_storage_impl(
        self,
        storage: Storage,
        *,
        no_empty_blobs: bool = False,
        skip_empty_writes: bool = False,
        # To make testing more meaningful, it's useful to make sure that
        # some writes fill up any output buffers.  For filesystem writes
        # from Python, this default is probably enough.
        mul: int = 314159,  # just about 300KB
        # If the blob-store has a read-through cache, we cannot effectively
        # test that the remove actually happened.
        remove_is_immediate: bool = True,
    ) -> List[Tuple[List[str], str]]:  # Writes + their storage ID
        # Make sure nothing bad happens if an exception flies before a
        # commit.  Since we don't have an ID, we can't really test that the
        # partial write got discarded.
        with self.assertRaisesRegex(RuntimeError, "^humbug$"):
            # pyre-fixme[16]: `Storage` has no attribute `writer`.
            with storage.writer() as output:
                output.write(b"bah")
                raise RuntimeError("humbug")

        with self.assertRaisesRegex(AssertionError, "^Cannot commit twice$"):
            with storage.writer() as output:
                output.write(b"foo")
                output.commit(remove_on_exception=True)  # Leave no litter
                output.commit()

        # Check that the `remove_on_exception` kwarg triggers `remove`.
        mock_remove = MagicMock()
        with patch.object(storage, "remove", mock_remove):
            with self.assertRaisesRegex(RuntimeError, "^remove_on_exception$"):
                with storage.writer() as output:
                    output.write(b"foo")
                    id_to_remove = output.commit(remove_on_exception=True)
                    # Contract: committed blobs are available to read
                    with storage.reader(id_to_remove) as reader:
                        self.assertEqual(b"foo", reader.read())
                    raise RuntimeError("remove_on_exception")

        # Check that `remove` would have been called, and then call it.
        mock_remove.assert_called_once_with(id_to_remove)
        storage.remove(id_to_remove)  # Exercise the real `remove`
        if remove_is_immediate:
            # The removed ID should not longer be available.
            with self.assertRaises(Exception):
                with storage.reader(id_to_remove) as input:
                    # The reader may be a pipe from another Python process,
                    # let's consume its output to avoid BrokenPipe logspam.
                    input.read()

        return [
            (
                writes,
                self._check_write_and_read(
                    storage,
                    writes if i is None else [*writes[:i], b"", *writes[i:]],
                ),
            )
            for writes in [
                # Some large writes
                [b"abcd" * mul, b"efgh" * mul],
                [b"abc" * mul, b"defg" * mul],
                [b"abc" * mul, b"def" * mul, b"g" * mul],
                [b"abcd" * mul],
                [b"abc" * mul, b"d" * mul],
                # Some tiny writes without a multiplier
                [b"a", b"b", b"c", b"d"],
                [b"ab"],
                [b"a", b"b"],
                # While clowny, some blob storage systems refuse empty blobs.
                *([] if no_empty_blobs else [[b""], []]),
            ]
            # Test the given writes, optionally insert a blank at each pos
            for i in [
                None,
                *([] if skip_empty_writes else range(len(writes) + 1)),
            ]
        ]
