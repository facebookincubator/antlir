#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import hashlib
import sys
import time
import traceback
from concurrent.futures import ThreadPoolExecutor, as_completed
from functools import partial
from types import MappingProxyType
from typing import Dict, FrozenSet, Iterable, Iterator, Set, Tuple

from fs_image.common import get_file_logger, shuffled
from fs_image.rpm.common import read_chunks, retryable
from fs_image.rpm.db_connection import DBConnectionContext
from fs_image.rpm.downloader.common import (
    BUFFER_BYTES,
    DownloadConfig,
    DownloadResult,
    LogOp,
    download_resource,
    log_size,
    maybe_write_id,
    retryable_db_ctx,
    timeit,
    verify_chunk_stream,
)
from fs_image.rpm.downloader.deleted_mutable_rpms import deleted_mutable_rpms
from fs_image.rpm.downloader.logger import log_sample
from fs_image.rpm.repo_db import RpmTable
from fs_image.rpm.repo_objects import CANONICAL_HASH, Checksum, Rpm
from fs_image.rpm.repo_snapshot import (
    HTTPError,
    MaybeStorageID,
    MutableRpmError,
    ReportableError,
)
from fs_image.rpm.yum_dnf_conf import YumDnfConfRepo


RPM_MAX_RETRY_S = [2 ** i for i in range(9)]  # 512 sec ==  8m32s
log = get_file_logger(__file__)


def _is_retryable_http_err(e: Exception) -> bool:
    if not isinstance(e, HTTPError):
        return False
    # 408 is 'Request Timeout' and, as with 5xx, can reasonably be presumed
    # to be a transient issue that's worth retrying
    status = e.to_dict()["http_status"]
    return status // 100 == 5 or status == 408


def _detect_mutable_rpms(
    rpm: Rpm,
    rpm_table: RpmTable,
    storage_id: str,
    all_snapshot_universes: Set[str],
    db_conn: DBConnectionContext,
) -> MaybeStorageID:
    with retryable_db_ctx(db_conn) as repo_db_ctx:
        all_canonical_checksums = set(
            repo_db_ctx.get_rpm_canonical_checksums(
                rpm_table, rpm, all_snapshot_universes
            )
        )
    assert all_canonical_checksums, (rpm, storage_id)
    assert all(
        c.algorithm == CANONICAL_HASH for c in all_canonical_checksums
    ), all_canonical_checksums
    all_canonical_checksums.remove(rpm.canonical_checksum)
    deleted_checksums = deleted_mutable_rpms.get(rpm.nevra(), set())
    assert (
        rpm.canonical_checksum not in deleted_checksums
    ), f"{rpm} was in deleted_mutable_rpms, but still exists in repos"
    all_canonical_checksums.difference_update(deleted_checksums)
    if all_canonical_checksums:
        # Future: It would be nice to mark all mentions of the NEVRA
        # as bad, but that requires messy updates of multiple
        # `RepoSnapshot`s.  For now, we rely on the fact that the next
        # `snapshot-repos` run will do this anyway.
        return MutableRpmError(
            location=rpm.location,
            storage_id=storage_id,
            checksum=rpm.canonical_checksum,
            other_checksums=all_canonical_checksums,
        )
    return storage_id


# May raise `ReportableError`s to be caught by `_download_rpms`.
# May raise an `HTTPError` if the download fails, which won't trigger a
# retry if they're not 5xx/408 errors.
@retryable(
    "Download failed: {rpm}",
    RPM_MAX_RETRY_S,
    is_exception_retryable=_is_retryable_http_err,
)
def _download_rpm(
    rpm: Rpm, repo_url: str, rpm_table: RpmTable, cfg: DownloadConfig
) -> Tuple[Rpm, str]:
    "Returns a storage_id and a copy of `rpm` with a canonical checksum."
    log.info(f"Downloading {rpm}")
    storage = cfg.new_storage()
    with download_resource(
        repo_url, rpm.location
    ) as input_, storage.writer() as output:
        # Before committing to the DB, let's standardize on one hash
        # algorithm.  Otherwise, it might happen that two repos may
        # store the same RPM hashed with different algorithms, and thus
        # trigger our "different hashes" detector for a sane RPM.
        canonical_hash = hashlib.new(CANONICAL_HASH)
        for chunk in verify_chunk_stream(
            read_chunks(input_, BUFFER_BYTES), [rpm.checksum], rpm.size, rpm.location
        ):  # May raise a ReportableError
            canonical_hash.update(chunk)
            output.write(chunk)
        # NB: We can also query the RPM as we download it above, via
        # something like P123285392.  However, at present, all necessary
        # metadata can be retrieved via `parse_metadata.py`.
        rpm = rpm._replace(
            canonical_checksum=Checksum(
                algorithm=CANONICAL_HASH, hexdigest=canonical_hash.hexdigest()
            )
        )
        storage_id = output.commit()
    assert storage_id is not None
    return rpm, storage_id


def _handle_rpm(
    rpm: Rpm,
    universe: str,
    repo_url: str,
    rpm_table: RpmTable,
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
) -> Tuple[Rpm, MaybeStorageID, float]:
    """Fetches the specified RPM from the repo DB and downloads it if needed.

    Returns a 3-tuple of the hydrated RPM, storage ID or exception if one was
    caught, and bytes downloaded, if a download occurred (used for reporting).
    """
    # Read-after-write consitency is not needed here as this is the first read
    # in the execution model. It's possible another concurrent snapshot is
    # running that could race with this read, but that's not critical as this
    # section should be idempotent, and at worst we'll duplicate some work by
    # re-downloading the RPM.
    with cfg.new_db_ctx(readonly=True, force_master=False) as ro_repo_db:
        # If we get no `storage_id` back, there are 3 possibilities:
        #  - `rpm.nevra()` was never seen before.
        #  - `rpm.nevra()` was seen before, but it was hashed with
        #     different algorithm(s), so we MUST download and
        #     compute the canonical checksum to know if its contents
        #     are the same.
        #  - `rpm.nevra()` was seen before, **AND** one of the
        #    prior checksums used `rpm.checksum.algorithms`, but
        #    produced a different hash value.  In other words, this
        #    is a `MutableRpmError`, because the same NEVRA must
        #    have had two different contents.  We COULD explicitly
        #    detect this error here, and avoid the download.
        #    However, this severe error should be infrequent, and we
        #    actually get valuable information from the download --
        #    this lets us know whether the file is wrong or the
        #    repodata is wrong.
        with timeit(partial(log_sample, LogOp.RPM_QUERY, rpm=rpm, universe=universe)):
            storage_id, canonical_chk = ro_repo_db.get_rpm_storage_id_and_checksum(
                rpm_table, rpm
            )
    # If the RPM is already stored with a matching checksum, just update its
    # `.canonical_checksum`. Note that `rpm` was parsed from repodata, and thus
    # it's guaranteed to not yet have a `canonical_checksum`.
    if storage_id:
        rpm = rpm._replace(canonical_checksum=canonical_chk)
        # This is a very common case and thus noisy log, so we write to debug
        log.debug(f"Already stored under {storage_id}: {rpm}")
        return rpm, storage_id, 0
    # We have to download the RPM.
    try:
        with timeit(
            partial(log_sample, LogOp.RPM_DOWNLOAD, rpm=rpm, universe=universe)
        ):
            rpm, storage_id = _download_rpm(rpm, repo_url, rpm_table, cfg)
            return rpm, storage_id, rpm.size
    # RPM checksum validation errors, HTTP errors, etc
    except ReportableError as ex:
        # This "fake" storage_id is stored in `storage_id_to_rpm`, so the
        # error is propagated to sqlite db through the snapshot. It isn't
        # written to repo_db however as that happens in the *_impl function
        return rpm, ex, 0


def _download_rpms(
    repo: YumDnfConfRepo,
    universe: str,
    rpm_table: RpmTable,
    rpms: Iterable[Rpm],
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
) -> Tuple[Dict[MaybeStorageID, Rpm], float]:
    storage_id_to_rpm = {}
    duplicate_rpms = 0
    rw_db_conn = cfg.new_db_conn(readonly=False)
    ro_db_conn = cfg.new_db_conn(readonly=True)
    total_bytes_downloaded = 0
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(
                _handle_rpm,
                rpm,
                universe,
                repo.base_url,
                rpm_table,
                all_snapshot_universes,
                cfg,
            )
            # Download in random order to reduce collisions from racing writers.
            for rpm in shuffled(rpms)
            if cfg.rpm_shard.in_shard(rpm)
        ]
        for future in as_completed(futures):
            rpm, res_storage_id, bytes_dl = future.result()
            total_bytes_downloaded += bytes_dl
            if not isinstance(res_storage_id, ReportableError):
                # If it's valid, we store this storage_id in repo_db regardless of
                # whether we encounter fatal errors later on that fail the snapshot;
                # see docblock in `repo_downloader.py` for reasoning
                with timeit(
                    partial(log_sample, LogOp.REPO_DB_WRITE, rpm=rpm, universe=universe)
                ):
                    res_storage_id = maybe_write_id(
                        rpm, res_storage_id, rpm_table, rw_db_conn
                    )
                # Detect if this RPM NEVRA occurs with different contents.
                with timeit(
                    partial(
                        log_sample,
                        LogOp.DETECT_MUTABLE_RPMS,
                        rpm=rpm,
                        universe=universe
                    )
                ):
                    res_storage_id = _detect_mutable_rpms(
                        rpm,
                        rpm_table,
                        res_storage_id,
                        all_snapshot_universes,
                        ro_db_conn
                    )
            existing_rpm = storage_id_to_rpm.get(res_storage_id)
            if existing_rpm and existing_rpm != rpm:  # pragma: no cover
                duplicate_rpms += 1
                message = (
                    f'Same ID {res_storage_id} with differing RPMs: '
                    f'{existing_rpm} != {rpm}'
                )
                # We don't care if locations diverge because we only need a single
                # location for a NEVRA to be able to fetch the RPM.
                if existing_rpm._replace(location=None) == rpm._replace(location=None):
                    log.warning(message)
                else:
                    raise RuntimeError(message)
            storage_id_to_rpm[res_storage_id] = rpm

    assert (
       len(storage_id_to_rpm)
       == (sum(cfg.rpm_shard.in_shard(r) for r in rpms) - duplicate_rpms)
    )
    return storage_id_to_rpm, total_bytes_downloaded


def gen_rpms_from_repodatas(
    repodata_results: Iterable[DownloadResult],
    cfg: DownloadConfig,
    all_snapshot_universes: FrozenSet[str],
) -> Iterator[DownloadResult]:
    for res in repodata_results:
        repo_weight_bytes = sum(r.size for r in res.rpms)
        num_rpms = len(res.rpms)
        log_size(f"`{res.repo.name}` has {num_rpms} RPMs weighing", repo_weight_bytes)
        start_t = time.time()
        total_dl = 0
        try:
            storage_id_to_rpm, total_dl = _download_rpms(
                res.repo,
                res.repo_universe,
                RpmTable(res.repo_universe),
                res.rpms,
                all_snapshot_universes,
                cfg,
            )
            yield res._replace(storage_id_to_rpm=MappingProxyType(storage_id_to_rpm))
        finally:
            log_sample(
                LogOp.REPO_DOWNLOAD,
                duration_s=time.time() - start_t,
                universe=res.repo_universe,
                repo_name=res.repo.name,
                repo_num_rpms=num_rpms,
                repo_downloaded_gb=total_dl / 10**9,
                repo_weight_gb=repo_weight_bytes / 10 ** 9,
                error=traceback.format_exc() if any(sys.exc_info()) else None,
            )
