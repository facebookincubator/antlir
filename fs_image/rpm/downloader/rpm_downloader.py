#!/usr/bin/env python3
import hashlib
from concurrent.futures import ThreadPoolExecutor, as_completed
from types import MappingProxyType
from typing import (
    FrozenSet, Iterable, Iterator, Set, Tuple
)

import MySQLdb

from fs_image.common import get_file_logger, set_new_key, shuffled
from fs_image.rpm.downloader.common import (
    BUFFER_BYTES, DownloadConfig, DownloadResult, download_resource, log_size,
    maybe_write_id, verify_chunk_stream
)
from fs_image.rpm.downloader.deleted_mutable_rpms import deleted_mutable_rpms
from rpm.common import read_chunks, retryable
from rpm.repo_db import RepoDBContext, RpmTable
from rpm.repo_objects import CANONICAL_HASH, Checksum, Rpm
from rpm.repo_snapshot import (
    HTTPError, MaybeStorageID, MutableRpmError, ReportableError
)
from rpm.yum_dnf_conf import YumDnfConfRepo

RPM_MAX_RETRY_S = [2 ** i for i in range(9)]  # 512 sec ==  8m32s
DB_MAX_RETRY_S = [2 ** i for i in range(8)]  # 256 sec == 4m16s
log = get_file_logger(__file__)


def _is_retryable_http_err(e: Exception):
    if not isinstance(e, HTTPError):
        return False
    # 408 is 'Request Timeout' and, as with 5xx, can reasonably be presumed
    # to be a transient issue that's worth retrying
    status = e.to_dict()['http_status']
    return status // 100 == 5 or status == 408


def _is_retryable_mysql_err(e: Exception):  # pragma: no cover
    return isinstance(e, MySQLdb.OperationalError)


def _detect_mutable_rpms(
    rpm: Rpm,
    rpm_table: RpmTable,
    storage_id: str,
    all_snapshot_universes: Set[str],
    db_ctx: RepoDBContext,
) -> MaybeStorageID:
    with db_ctx as repo_db_ctx:
        all_canonical_checksums = set(repo_db_ctx.get_rpm_canonical_checksums(
            rpm_table, rpm, all_snapshot_universes,
        ))
    assert all_canonical_checksums, (rpm, storage_id)
    assert all(
        c.algorithm == CANONICAL_HASH for c in all_canonical_checksums
    ), all_canonical_checksums
    all_canonical_checksums.remove(rpm.canonical_checksum)
    deleted_checksums = deleted_mutable_rpms.get(rpm.nevra(), set())
    assert rpm.canonical_checksum not in deleted_checksums, \
        f'{rpm} was in deleted_mutable_rpms, but still exists in repos'
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
    'Download failed: {rpm}',
    RPM_MAX_RETRY_S,
    is_exception_retryable=_is_retryable_http_err
)
def _download_rpm(
    rpm: Rpm,
    repo_url: str,
    rpm_table: RpmTable,
    cfg: DownloadConfig,
) -> Tuple[Rpm, str]:
    'Returns a storage_id and a copy of `rpm` with a canonical checksum.'
    log.info(f'Downloading {rpm}')
    storage = cfg.new_storage()
    with download_resource(repo_url, rpm.location) as input_, \
            storage.writer() as output:
        # Before committing to the DB, let's standardize on one hash
        # algorithm.  Otherwise, it might happen that two repos may
        # store the same RPM hashed with different algorithms, and thus
        # trigger our "different hashes" detector for a sane RPM.
        canonical_hash = hashlib.new(CANONICAL_HASH)
        for chunk in verify_chunk_stream(
            read_chunks(input_, BUFFER_BYTES),
            [rpm.checksum],
            rpm.size,
            rpm.location,
        ):  # May raise a ReportableError
            canonical_hash.update(chunk)
            output.write(chunk)
        # NB: We can also query the RPM as we download it above, via
        # something like P123285392.  However, at present, all necessary
        # metadata can be retrieved via `parse_metadata.py`.
        rpm = rpm._replace(canonical_checksum=Checksum(
            algorithm=CANONICAL_HASH, hexdigest=canonical_hash.hexdigest(),
        ))
        storage_id = output.commit()
    assert storage_id is not None
    return rpm, storage_id


@retryable(
    'Exception while querying database for {rpm}',
    DB_MAX_RETRY_S,
    is_exception_retryable=_is_retryable_mysql_err,
)
def _get_rpm_storage_id_and_checksum(
    rpm_table: RpmTable,
    rpm: Rpm,
    cfg: DownloadConfig,
) -> Tuple[str, Checksum]:
    with cfg.new_db_ctx(readonly=True) as ro_repo_db:
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
        return ro_repo_db.get_rpm_storage_id_and_checksum(rpm_table, rpm)


def _handle_rpm(
    rpm: Rpm,
    repo_url: str,
    rpm_table: RpmTable,
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
) -> Tuple[Rpm, MaybeStorageID]:
    storage_id, canonical_chk = _get_rpm_storage_id_and_checksum(
        rpm_table, rpm, cfg
    )
    # If the RPM is already stored with a matching checksum, just update its
    # `.canonical_checksum`.
    if storage_id:
        rpm = rpm._replace(canonical_checksum=canonical_chk)
        # This is a very common case and thus noisy log, so we write to debug
        log.debug(f'Already stored under {storage_id}: {rpm}')
        return rpm, storage_id
    # We have to download the RPM.
    try:
        return _download_rpm(rpm, repo_url, rpm_table, cfg)
    # RPM checksum validation errors, HTTP errors, etc
    except ReportableError as ex:
        # This "fake" storage_id is stored in `storage_id_to_rpm`, so the error
        # is propagated to sqlite db through the snapshot. It isn't written to
        # repo_db however as that happens in the *_impl function
        return rpm, ex


def _download_rpms(
    repo: YumDnfConfRepo,
    rpm_table: RpmTable,
    rpms: Iterable[Rpm],
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
):
    log_size(
        f'`{repo.name}` has {len(rpms)} RPMs weighing',
        sum(r.size for r in rpms)
    )
    storage_id_to_rpm = {}
    rw_db_ctx = cfg.new_db_ctx(readonly=False)
    ro_db_ctx = cfg.new_db_ctx(readonly=True)
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(
                _handle_rpm,
                rpm,
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
            rpm, res_storage_id = future.result()
            # If it's valid, we store this storage_id to repo_db regardless of
            # whether we encounter fatal errors later on in the execution and
            # don't finish the snapshot - see top-level docblock for reasoning
            storage_id_or_err = maybe_write_id(
                rpm, res_storage_id, rpm_table, rw_db_ctx
            )
            # Detect if this RPM NEVRA occurs with different contents.
            if not isinstance(storage_id_or_err, ReportableError):
                storage_id_or_err = _detect_mutable_rpms(
                    rpm, rpm_table, storage_id_or_err, all_snapshot_universes,
                    ro_db_ctx
                )
            set_new_key(storage_id_to_rpm, storage_id_or_err, rpm)

    assert len(storage_id_to_rpm) == sum(
        cfg.rpm_shard.in_shard(r) for r in rpms
    )
    return storage_id_to_rpm


def gen_rpms_from_repodatas(
    repodata_results: Iterable[DownloadResult],
    cfg: DownloadConfig,
    all_snapshot_universes: FrozenSet[str],
) -> Iterator[DownloadResult]:
    for res in repodata_results:
        storage_id_to_rpm = _download_rpms(
            res.repo,
            RpmTable(res.repo_universe),
            res.rpms,
            all_snapshot_universes,
            cfg,
        )
        yield res._replace(
            storage_id_to_rpm=MappingProxyType(storage_id_to_rpm)
        )
