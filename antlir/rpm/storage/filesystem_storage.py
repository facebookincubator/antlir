#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import stat
import uuid
from contextlib import contextmanager
from typing import AnyStr, Iterator

from antlir.fs_utils import Path

from antlir.rpm.storage.storage import (
    _CommitCallback,
    Storage,
    StorageInput,
    StorageOutput,
)


class FilesystemStorage(Storage, plugin_kind="filesystem"):
    """
    Stores blobs on the local filesystem. This is great if you initially
    just want to commit RPMs to a local SVN (or similar) repo.

    Once you end up having too many RPMs for filesystem storage, you can
    write a similar plugin for your favorite "key -> large binary object"
    distributed store, and migrate there.
    """

    def __init__(self, *, key: str, base_dir: AnyStr) -> None:
        self.key = key
        self.base_dir = Path(base_dir).abspath()

    def _path_for_storage_id(self, sid: str) -> str:
        """
        A hierarchy 4 levels deep with a maximum of 4096 subdirs per dir.
        You'd need about 300 trillion blobs before the leaf subdirs have an
        average of 4096 subdirs each.
        """
        return self.base_dir / sid[:3] / sid[3:6] / sid[6:9] / sid[9:]

    @contextmanager
    def writer(self) -> Iterator[StorageOutput]:
        sid = str(uuid.uuid4()).replace("-", "")
        sid_path = self._path_for_storage_id(sid)
        try:
            # pyre-fixme[16]: `str` has no attribute `dirname`.
            os.makedirs(sid_path.dirname())
        except FileExistsError:  # pragma: no cover
            pass

        with os.fdopen(
            os.open(
                sid_path,
                os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC,
                mode=stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH,
            ),
            "wb",
        ) as outfile:

            @contextmanager
            def get_id_and_release_resources():
                try:
                    yield sid
                finally:
                    # This `close()` flushes, making the written data readable,
                    # and prevents more writes via `StorageOutput`.
                    outfile.close()

            # `_CommitCallback` has a `try` to clean up on error. This
            # placement of the context assumes that `os.fdopen` cannot fail.
            with _CommitCallback(self, get_id_and_release_resources) as commit:
                yield StorageOutput(output=outfile, commit_callback=commit)

    @contextmanager
    def reader(self, sid: str) -> Iterator[StorageInput]:
        with open(self._path_for_storage_id(self.strip_key(sid)), "rb") as inp:
            yield StorageInput(input=inp)

    def remove(self, sid: str) -> None:
        sid_path = self._path_for_storage_id(self.strip_key(sid))
        assert sid_path.startswith(self.base_dir + b"/")
        os.remove(sid_path)
        # Remove any empty directories up to `self.filesystem_path`.
        # pyre-fixme[16]: `str` has no attribute `dirname`.
        dir_path = sid_path.dirname()
        while dir_path != self.base_dir:
            try:
                os.rmdir(dir_path)
            except OSError:  # pragma: no cover
                break
            dir_path = dir_path.dirname()
