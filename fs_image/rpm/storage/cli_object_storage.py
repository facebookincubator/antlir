#!/usr/bin/env python3
import subprocess
import uuid

from abc import abstractmethod
from contextlib import contextmanager
from typing import ContextManager, List, Mapping, NamedTuple

from rpm.common import check_popen_returncode, get_file_logger
from .storage import _CommitCallback, Storage, StorageInput, StorageOutput

log = get_file_logger(__file__)


class _StorageRemover(NamedTuple):
    storage: Storage
    procs: List[subprocess.Popen]

    def remove(self, sid: str) -> None:
        self.procs.append(
            subprocess.Popen(
                self.storage._cmd(
                    path=self.storage._path_for_storage_id(
                        self.storage.strip_key(sid)
                    ),
                    operation="remove",
                ),
                env=self.storage._configured_env(),
                stdout=2
            )
        )


class CLIObjectStorage(Storage):

    @abstractmethod
    def _path_for_storage_id(self, sid: str) -> str:
        ...

    @abstractmethod
    def _cmd(self, *args, path: str, operation: str) -> str:
        ...

    @abstractmethod
    def _configured_env(self) -> Mapping:
        ...

    # Separate function so the unit-test can mock it.
    @classmethod
    def _make_storage_id(cls) -> str:
        return str(uuid.uuid4()).replace('-', '')

    @contextmanager
    def writer(self) -> ContextManager[StorageOutput]:
        sid = self._make_storage_id()

        with subprocess.Popen(self._cmd(
            # Read from stdin -- I assume `cli` does not use it.
            path=self._path_for_storage_id(sid),
            operation="write",
        ), env=self._configured_env(), stdin=subprocess.PIPE, stdout=2) as proc:

            @contextmanager
            def get_id_and_release_resources():
                # Wait for `cli` to exit cleanly to make sure the
                # `sid` is available to read after the `yield`.
                try:
                    proc.stdin.close()
                    proc.wait()
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
                        subprocess.run(['setsid'] + self._cmd(
                            path=self._path_for_storage_id(sid),
                            operation="remove",
                        ), env=self._configured_env(), stdout=2)
                    # To cover this, I'd need `setsid` or `cli` not to
                    # exist, neither is a useful test.  The validity of the
                    # f-string is ensured by `flake8`.
                    except Exception:  # pragma: no cover
                        # Log & ignore: we'll re-raise the original exception
                        log.exception(f'While cleaning up partial {sid}')
                    raise
                yield sid

            with _CommitCallback(self, get_id_and_release_resources) as commit:
                yield StorageOutput(output=proc.stdin, commit_callback=commit)

    @contextmanager
    def reader(self, sid: str) -> ContextManager[StorageInput]:
        with subprocess.Popen(self._cmd(
            path=self._path_for_storage_id(self.strip_key(sid)),
            operation="read",
        ), env=self._configured_env(), stdout=subprocess.PIPE) as proc:
            yield StorageInput(input=proc.stdout)
        check_popen_returncode(proc)

    @contextmanager
    def remover(self) -> ContextManager[_StorageRemover]:
        rm = _StorageRemover(storage=self, procs=[])
        try:
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
                # tested this by hand in an interpreter.
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
        with self.remover() as rm:
            rm.remove(sid)
