#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest.mock

from .. import storage
from .storage_base_test import Storage, StorageBaseTestCase


class CLIObjectStorageBaseTestCase(StorageBaseTestCase):
    def _test_write_and_read_back(self, storage_type: Storage) -> None:
        # pyre-fixme[16]: `Storage` has no attribute `_make_storage_id`.
        old_make_storage_id = storage_type._make_storage_id
        with unittest.mock.patch.object(
            storage_type,
            "_make_storage_id",
            # pyre-fixme[16]: `CLIObjectStorageBaseTestCase` has no attribute
            #  `_decorate_id`.
            side_effect=lambda: self._decorate_id(old_make_storage_id()),
        ):
            # Since our current implementation doesn't support the removal of
            # multiple keys in one command, eagerly start each remove and let it
            # run in the background while the test proceeds.
            #
            # pyre-fixme[16]: `CLIObjectStorageBaseTestCase` has no attribute
            # `storage`.
            with self.storage.remover() as rm:
                # CLI startup can take a while to start (~0.8 seconds in the
                # case of manifold), so the network time to upload a bunch of
                # blobs 2-10MB in size is negligible. We test with bigger
                # uploads so that we're more likely to fill up any
                # intermittent buffers.
                for _, sid in self.check_storage_impl(
                    self.storage, mul=1_234_567, skip_empty_writes=False
                ):
                    rm.remove(sid)

    # pyre-fixme[3]: Return type must be annotated.
    def _test_uncommited(self, storage_type: Storage):
        # pyre-fixme[16]: `CLIObjectStorageBaseTestCase` has no attribute
        #  `_decorate_id`.
        # pyre-fixme[16]: `Storage` has no attribute `_make_storage_id`.
        fixed_sid = self._decorate_id(storage_type._make_storage_id())
        with unittest.mock.patch.object(
            storage_type, "_make_storage_id", return_value=fixed_sid
        ) as mock:
            with self.assertRaisesRegex(RuntimeError, "^abracadabra$"):
                # pyre-fixme[16]: `CLIObjectStorageBaseTestCase` has no
                #  attribute `storage`.
                with self.storage.writer() as out:
                    out.write(b"boohoo")
                    # Test our exception-before-commit handling
                    raise RuntimeError("abracadabra")

            with self.storage.writer() as out:
                # No commit, so this will not get written!
                out.write(b"foobar")

            self.assertEqual([(), ()], mock.call_args_list)

            proc = subprocess.run(
                self.storage._exists_cmd(
                    path=self.storage._path_for_storage_id(fixed_sid)
                ),
                env=self.storage._configured_env(),
                stdout=subprocess.PIPE,
            )
            self.assertEqual(1, proc.returncode)

            return proc

    # pyre-fixme[2]: Parameter must be annotated.
    def _test_error_cleanup(self, storage_kind: str, **kwargs) -> None:
        # Without a commit, all our failed cleanup is "behind the
        # scenes", and even though it errors and logs, it does not raise
        # an externally visible exception:
        with self.assertLogs(storage.__name__, level="ERROR") as cm:
            # pyre-fixme[16]: `Pluggable` has no attribute `writer`.
            with Storage.make(
                key="test", kind=storage_kind, **kwargs
            ).writer() as out:
                out.write(b"triggers error cleanup via commit-to-delete")
        (msg,) = cm.output
        self.assertRegex(msg, r"Error retrieving ID .* uncommitted blob\.")

        # If we do try to commit, error from the underlying CLI will be raised.
        with Storage.make(
            key="test", kind=storage_kind, **kwargs
        ).writer() as out:
            out.write(b"something")
            with self.assertRaises(subprocess.CalledProcessError):
                out.commit()
