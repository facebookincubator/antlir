#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import uuid
from abc import abstractmethod
from contextlib import contextmanager
from typing import ContextManager, List, Mapping, NamedTuple

from antlir.common import check_popen_returncode, get_logger

# Module import ensures we get plugins
from antlir.rpm.storage import Storage, StorageInput, StorageOutput

# Below not exported at the module level
from antlir.rpm.storage.storage import _CommitCallback


log = get_logger()


class _StorageRemover(NamedTuple):
    storage: Storage
    procs: List[subprocess.Popen]

    def remove(self, sid: str) -> None:
        self.procs.append(
            subprocess.Popen(
                # pyre-fixme[16]: `Storage` has no attribute `_remove_cmd`.
                self.storage._remove_cmd(
                    # pyre-fixme[16]: `Storage` has no attribute
                    # `_path_for_storage_id`.
                    path=self.storage._path_for_storage_id(self.storage.strip_key(sid))
                ),
                # pyre-fixme[16]: `Storage` has no attribute `_configured_env`.
                env=self.storage._configured_env(),
                stdout=2,
            )
        )


class CLIObjectStorage(Storage):
    """
    This abstract base class exists because most blob-store CLIs will
    behave very similarly, and can reuse the plumbing that turns them
    into `Storage` objects. Look at `S3Storage` for a concrete example.
    """

    @abstractmethod
    def _path_for_storage_id(self, sid: str) -> str:
        ...  # pragma: no cover

    @abstractmethod
    def _read_cmd(self, *args, path: str) -> List[str]:
        ...  # pragma: no cover

    @abstractmethod
    def _write_cmd(self, *args, path: str) -> List[str]:
        ...  # pragma: no cover

    @abstractmethod
    def _remove_cmd(self, *args, path: str) -> List[str]:
        ...  # pragma: no cover

    @abstractmethod
    def _exists_cmd(self, *args, path: str) -> List[str]:
        ...  # pragma: no cover

    @abstractmethod
    def _configured_env(self) -> Mapping:
        ...  # pragma: no cover

    # Separate function so the unit-test can mock it.
    @classmethod
    def _make_storage_id(cls) -> str:
        return str(uuid.uuid4()).replace("-", "")

    @contextmanager
    def writer(self) -> ContextManager[StorageOutput]:
        sid = self._make_storage_id()
        path = self._path_for_storage_id(sid)
        log_prefix = f"{self.__class__.__name__}"
        log.debug(f"{log_prefix} - Writing to {path}")
        with subprocess.Popen(
            self._write_cmd(
                # The underlying CLI is expected to read the blob from stdin
                path=path
            ),
            env=self._configured_env(),
            stdin=subprocess.PIPE,
            stdout=2,
        ) as proc:

            @contextmanager
            def get_id_and_release_resources():
                # Wait for `cli` to exit cleanly to make sure the
                # `sid` is available to read after the `yield`.
                try:
                    proc.stdin.close()
                    log.debug(f"{log_prefix} - Wait for {path} PUT")
                    proc.wait()
                    log.debug(
                        f"{log_prefix} - Exit code {proc.returncode}"
                        f" from {path} PUT"
                    )
                    check_popen_returncode(proc)
                # Clean up even on KeyboardInterrupt -- we cannot assume
                # that the blob was stored unless `cli` exited cleanly.
                #
                # The reason we need this clunky `except` is that we never
                # pass the sid to `_CommitCallback`, so it can't clean up.
                except BaseException:
                    try:
                        # Daemonize the cleanup: do NOT wait, do not check
                        # the return code.  Future: Following the idea in
                        # remove(), we could plop this cleanup on the
                        # innermost remover.
                        subprocess.run(
                            ["setsid"]
                            + self._remove_cmd(path=self._path_for_storage_id(sid)),
                            env=self._configured_env(),
                            stdout=2,
                        )
                    # To cover this, I'd need `setsid` or `cli` not to
                    # exist, neither is a useful test.  The validity of the
                    # f-string is ensured by `flake8`.
                    except Exception:  # pragma: no cover
                        # Log & ignore: we'll re-raise the original exception
                        log.exception(f"{log_prefix} - While cleaning up partial {sid}")
                    raise
                yield sid

            # pyre-fixme[6]: Expected `ContextManager[typing.Any]` for 2nd
            # param but got `() -> Any`.
            with _CommitCallback(self, get_id_and_release_resources) as commit:
                # pyre-fixme[7]: Expected
                # `ContextManager[antlir.rpm.storage.storage.StorageOutput]`
                # but got `Generator[antlir.rpm.storage.storage.StorageOutput,
                # None, None]`.
                # pyre-fixme[6]: Expected `IO[typing.Any]` for 1st param but got
                #  `Optional[typing.IO[typing.Any]]`.
                yield StorageOutput(output=proc.stdin, commit_callback=commit)

    @contextmanager
    def reader(self, sid: str) -> ContextManager[StorageInput]:
        # We currently waste significant time per read waiting for CLIs
        # to start, which is terrible for small reads (most system
        # RPMs are small).
        path = self._path_for_storage_id(self.strip_key(sid))
        log_prefix = f"{self.__class__.__name__}"
        with subprocess.Popen(
            self._read_cmd(path=path),
            env=self._configured_env(),
            stdout=subprocess.PIPE,
        ) as proc:
            log.debug(f"{log_prefix} - Started {path} GET proc")
            # pyre-fixme[7]: Expected
            #  `ContextManager[antlir.rpm.storage.storage.StorageInput]` but got
            #  `Generator[antlir.rpm.storage.storage.StorageInput, None, None]`.
            # pyre-fixme[6]: Expected `IO[typing.Any]` for 1st param but got
            #  `Optional[typing.IO[typing.Any]]`.
            yield StorageInput(input=proc.stdout)
            log.debug(f"{log_prefix} - Waiting for {path} GET")
        log.debug(f"{log_prefix} - Exit code {proc.returncode} from  {path} GET")
        # No `finally`: this doesn't need to run if the context block raises.
        check_popen_returncode(proc)

    @contextmanager
    def remover(self) -> ContextManager[_StorageRemover]:
        rm = _StorageRemover(storage=self, procs=[])
        try:
            # pyre-fixme[7]: Expected `ContextManager[_StorageRemover]` but got
            #  `Generator[_StorageRemover, None, None]`.
            yield rm
        finally:
            last_ex = None  # We'll re-raise the last exception.
            for proc in rm.procs:
                # Ensure we wait for each process, no matter what.
                try:
                    assert proc.returncode is None  # Not yet waited for
                    proc.wait()
                # Unit-testing this error-within-error case is hard, but all
                # it would verify is that we properly re-raise `ex`.  I
                # tested this by hand in an interpreter, see P60127851.
                except Exception as ex:  # pragma: no cover
                    last_ex = ex
            # Raise the **last** of the "wait()" exceptions.
            if last_ex is not None:  # pragma: no cover
                raise last_ex
            # Check return codes after all processes have been waited for to
            # avoid creating zombies in the event that the caller catches.
            for proc in rm.procs:
                check_popen_returncode(proc)

    def remove(self, sid: str) -> None:
        # Future: for automatic removes, we could improve latency by placing
        # them into the innermost `remover`.  This would require us to keep
        # a `remover` stack on hand, with the outermost remover attached to
        # the Storage itself (maybe make it a context manager?).
        with self.remover() as rm:
            rm.remove(sid)
