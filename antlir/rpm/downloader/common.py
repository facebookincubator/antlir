#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
import time
import traceback
import urllib.parse
from contextlib import contextmanager
from enum import auto, Enum
from io import BytesIO
from typing import (
    Callable,
    ContextManager,
    Dict,
    FrozenSet,
    Iterable,
    Iterator,
    Mapping,
    NamedTuple,
    Optional,
    Union,
)

import requests
from antlir.common import get_logger, retryable
from antlir.rpm.common import DecorateContextEntry, RpmShard
from antlir.rpm.db_connection import DBConnectionContext
from antlir.rpm.open_url import open_url
from antlir.rpm.repo_db import RepoDBContext, StorageTable
from antlir.rpm.repo_objects import Checksum, Repodata, RepoMetadata, Rpm
from antlir.rpm.repo_snapshot import FileIntegrityError, HTTPError, MaybeStorageID
from antlir.rpm.storage import Storage
from antlir.rpm.yum_dnf_conf import YumDnfConfRepo


# We'll download data in 512KB chunks. This needs to be reasonably large to
# avoid small-buffer overheads, but not too large, since we use `zlib` for
# incremental decompression in `parse_repodata.py`, and its API has a
# complexity bug that makes it slow for large INPUT_CHUNK/OUTPUT_CHUNK.
BUFFER_BYTES = 2**19
DB_MAX_RETRY_S = [2**i for i in range(8)]  # 255 sec == 4m15s
log = get_logger()


class LogOp(Enum):
    # pyre-fixme[20]: Argument `value` expected.
    RPM_DOWNLOAD = auto()
    # pyre-fixme[20]: Argument `value` expected.
    DETECT_MUTABLE_RPMS = auto()
    # pyre-fixme[20]: Argument `value` expected.
    REPO_DB_WRITE = auto()
    # pyre-fixme[20]: Argument `value` expected.
    RPM_QUERY = auto()
    # pyre-fixme[20]: Argument `value` expected.
    REPO_DOWNLOAD = auto()

    def __str__(self) -> str:
        return str(self.name)


def _is_retryable_mysql_err(e: Exception) -> bool:  # pragma: no cover
    # Want to catch MySQLdb.OperationalError, which indicates a potentially
    # transient error, but don't want to import MySQLdb here, as it isn't a
    # dependency of this module otherwise. This approach is tested in
    # rpm/facebook/tests/test_fb_rpm_downloader.py
    return (
        type(e).__module__ == "MySQLdb._exceptions"
        and type(e).__qualname__ == "OperationalError"
    )


def retryable_db_ctx(
    db_conn: DBConnectionContext,
) -> ContextManager[RepoDBContext]:
    return DecorateContextEntry(
        RepoDBContext(db_conn, db_conn.SQL_DIALECT),
        retryable(
            "DB connection error",
            DB_MAX_RETRY_S,
            is_exception_retryable=_is_retryable_mysql_err,
        ),
    )


# Lightweight configuration used by various parts of the download
class DownloadConfig(NamedTuple):
    db_cfg: Dict[str, str]
    storage_cfg: Dict[str, str]
    rpm_shard: RpmShard
    threads: int

    def new_db_conn(
        self, *, readonly: bool, force_master: bool = True
    ) -> DBConnectionContext:
        assert "readonly" not in self.db_cfg, "readonly is picked by the caller"
        assert "force_master" not in self.db_cfg, "force_master is picked by the caller"
        # pyre-fixme [9]: Technically could be any `Pluggable`, but we use it as
        # a DBConnectionContext
        conn_ctx: DBConnectionContext = DBConnectionContext.from_json(
            {**self.db_cfg, "readonly": readonly, "force_master": force_master}
        )
        return conn_ctx

    def new_db_ctx(self, **kwargs) -> ContextManager[RepoDBContext]:
        return retryable_db_ctx(self.new_db_conn(**kwargs))

    def new_storage(self):
        return Storage.from_json(self.storage_cfg)


# Gets incrementally populated throughout repo downloading; used to carry info
# through the concurrent downloads until the final repo snapshot is built
class DownloadResult(NamedTuple):
    repo: YumDnfConfRepo
    repo_universe: str
    repomd: RepoMetadata
    # Below 3 params are populated incrementally after the initial 3
    storage_id_to_repodata: Optional[Mapping[MaybeStorageID, Repodata]] = None
    storage_id_to_rpm: Optional[Mapping[MaybeStorageID, Rpm]] = None
    rpms: Optional[FrozenSet[Rpm]] = None


def verify_chunk_stream(
    chunks: Iterable[bytes],
    checksums: Iterable[Checksum],
    size: int,
    location: str,
):
    actual_size = 0
    hashers = [ck.hasher() for ck in checksums]
    for chunk in chunks:
        actual_size += len(chunk)
        for hasher in hashers:
            hasher.update(chunk)
        yield chunk
    if actual_size != size:
        raise FileIntegrityError(
            location=location,
            failed_check="size",
            expected=size,
            actual=actual_size,
        )
    for hash, ck in zip(hashers, checksums):
        if hash.hexdigest() != ck.hexdigest:
            raise FileIntegrityError(
                location=location,
                failed_check=ck.algorithm,
                expected=ck.hexdigest,
                actual=hash.hexdigest(),
            )


def _log_if_storage_ids_differ(obj, storage_id, db_storage_id) -> None:
    if db_storage_id != storage_id:
        log.warning(f"Another writer already committed {obj} at {db_storage_id}")


def log_size(what_str: str, total_bytes: float) -> None:
    log.info(f"{what_str} {total_bytes/10**9:,.4f} GB")


@contextmanager
def timeit(callback: Callable):
    """`callback` should be a function that accepts kwargs `duration_s` and `error`,
    which will be called when the context manager exits.
    """
    start_t = time.time()
    try:
        yield
    finally:
        duration = time.time() - start_t
        callback(
            duration_s=duration,
            error=traceback.format_exc() if any(sys.exc_info()) else None,
        )


@contextmanager
def download_resource(repo_url: str, relative_url: str) -> Iterator[BytesIO]:
    if not repo_url.endswith("/"):
        repo_url += "/"  # `urljoin` needs a trailing / to work right
    assert not relative_url.startswith("/")
    try:
        with open_url(urllib.parse.urljoin(repo_url, relative_url)) as input:
            yield input
    except requests.exceptions.HTTPError as ex:
        # E.g. we can see 404 errors if packages were deleted
        # without updating the repodata.
        #
        # Future: If we see lots of transient error status codes
        # in practice, we could retry automatically before
        # waiting for the next snapshot, but the complexity is
        # not worth it for now.
        raise HTTPError(location=relative_url, http_status=ex.response.status_code)


# Note that we use this function serially from the master thread after
# performing the downloads. This is because it's possible for SQLite to run
# into locking issues with many concurrent writers. Additionally, these writes
# are a minor portion of our overall execution time and thus we see negligible
# perf gains by parallelizing them.
def maybe_write_id(
    repo_obj: Union[Repodata, Rpm],
    storage_id: str,
    table: StorageTable,
    db_conn: DBConnectionContext,
) -> str:
    """Used to write a storage_id to repo_db after a download."""
    with retryable_db_ctx(db_conn) as repo_db_ctx:
        db_storage_id = repo_db_ctx.maybe_store(table, repo_obj, storage_id)
        repo_db_ctx.commit()
    _log_if_storage_ids_differ(repo_obj, storage_id, db_storage_id)
    return db_storage_id
