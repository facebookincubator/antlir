#!/usr/bin/env python3
import requests
import time
import urllib.parse
from contextlib import contextmanager
from io import BytesIO
from typing import (
    Dict, FrozenSet, Iterable, Iterator, Mapping, NamedTuple, Optional, Union
)

from fs_image.common import get_file_logger
from rpm.common import RpmShard
from rpm.db_connection import DBConnectionContext
from rpm.open_url import open_url
from rpm.repo_db import RepoDBContext, RepodataTable
from rpm.repo_objects import Checksum, Repodata, RepoMetadata, Rpm
from rpm.repo_snapshot import (
    FileIntegrityError, HTTPError, ReportableError, MaybeStorageID
)
from rpm.storage import Storage
from rpm.yum_dnf_conf import YumDnfConfRepo


# We'll download data in 512KB chunks. This needs to be reasonably large to
# avoid small-buffer overheads, but not too large, since we use `zlib` for
# incremental decompression in `parse_repodata.py`, and its API has a
# complexity bug that makes it slow for large INPUT_CHUNK/OUTPUT_CHUNK.
BUFFER_BYTES = 2 ** 19
log = get_file_logger(__file__)


# Lightweight configuration used by various parts of the download
class DownloadConfig(NamedTuple):
    db_cfg: Dict[str, str]
    storage_cfg: Dict[str, str]
    rpm_shard: RpmShard
    threads: int

    def new_db_conn(self, *, readonly: bool):
        assert 'readonly' not in self.db_cfg, 'readonly is picked by the caller'
        return DBConnectionContext.from_json({
            **self.db_cfg,
            'readonly': readonly,
        })

    def new_db_ctx(self, *, readonly: bool):
        db_conn = self.new_db_conn(readonly=readonly)
        return RepoDBContext(db_conn, db_conn.SQL_DIALECT)

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
    chunks: Iterable[bytes], checksums: Iterable[Checksum], size: int,
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
            failed_check='size',
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


def _log_if_storage_ids_differ(obj, storage_id, db_storage_id):
    if db_storage_id != storage_id:
        log.warning(
            f'Another writer already committed {obj} at {db_storage_id}, '
            f'will delete our copy at {storage_id}'
        )


def log_size(what_str: str, total_bytes: int):
    log.info(f'{what_str} {total_bytes/10**9:,.4f} GB')


@contextmanager
def timeit(operation_msg: str, threshold_s: int):
    start_t = time.time()
    try:
        yield
    finally:
        duration = time.time() - start_t
        if duration > threshold_s:
            log.info(
                f'Operation exceeded threshold, {duration} > {threshold_s} secs'
            )


@contextmanager
def download_resource(repo_url: str, relative_url: str) -> Iterator[BytesIO]:
    if not repo_url.endswith('/'):
        repo_url += '/'  # `urljoin` needs a trailing / to work right
    assert not relative_url.startswith('/')
    try:
        full_url = urllib.parse.urljoin(repo_url, relative_url)
        with timeit(f'Downloading resource {full_url}', threshold_s=60 * 10), \
             open_url(full_url) as input:
            yield input
    except requests.exceptions.HTTPError as ex:
        # E.g. we can see 404 errors if packages were deleted
        # without updating the repodata.
        #
        # Future: If we see lots of transient error status codes
        # in practice, we could retry automatically before
        # waiting for the next snapshot, but the complexity is
        # not worth it for now.
        raise HTTPError(
            location=relative_url,
            http_status=ex.response.status_code,
        )


# Note that we use this function serially from the master thread after
# performing the downloads. This is because it's possible for SQLite to run
# into locking issues with many concurrent writers. Additionally, these writes
# are a minor portion of our overall execution time and thus we see negligible
# perf gains by parallelizing them.
def maybe_write_id(
    repo_obj: Union[Repodata, Rpm],
    storage_id: MaybeStorageID,
    table: RepodataTable,
    db_ctx: RepoDBContext,
):
    '''Used to write a storage_id to repo_db after a possible download.'''
    # Don't store errors into the repo db
    if isinstance(storage_id, ReportableError):
        return storage_id
    with timeit(f'Writing storage ID {storage_id}', threshold_s=10):
        with db_ctx as repo_db_ctx:
            db_storage_id = repo_db_ctx.maybe_store(table, repo_obj, storage_id)
            repo_db_ctx.commit()
    _log_if_storage_ids_differ(repo_obj, storage_id, db_storage_id)
    return db_storage_id
