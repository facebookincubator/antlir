#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
"Atomically" [1] downloads a snapshot of a sequence of RPM repos. Uses the
`repo_db.py` and `storage.py` abstractions to store the snapshots, while
avoiding duplication of RPMs that existed in prior snapshots.

Specifically, the user calls `download_repos(...)`, which, for each repo:
  - Downloads & parses `repomd.xml`.
  - Downloads the repodatas referenced there. Parses a primary repodata.
  - Downloads the RPMs referenced in the primary repodata.

To increase performance, each of the above steps is performed concurrently,
with `download_repos` being the driver that aggregates the thread results and
returns the final list of snapshots. Additionally, the single driver thread
performs all writes to mitigate potential concurrency issues with SQLite.

`download_repos` returns a list of `RepoSnapshot`s containing descriptions of
the stored objects. The dictionary keys are either "storage IDs" from the
supplied `Storage` class, or `ReportableError` instances for those that were
not correctly downloaded and stored. If a download fatally fails for a
particular repo (e.g. repomd download failed, or a primary repodata couldn't be
retrieved), the exception will be raised and the entire snapshot will fail.

Note that in the case of the snapshot failing part way through, we omit any
complex logic to clean up objects that have already been committed. This is
mainly because until the very last point of the snapshot, where repomds are
committed, these inserted repo objects should be unreferenced, and thus will
essentially just be taking up extra space in storage without causing any
integrity issues. Additionally, if this leaking becomes substantial, it's
possible to simply have a periodic clean-up job run which garbage collects any
unreferenced blobs - which is a much simpler approach compared to ensuring we
always clean up unfinished work.

[1] The snapshot is only atomic (i.e. representative of a single point in time,
as opposed to a sheared mix of the repo at various points in time) if:
  - Repodata files and RPM files are never mutated after creation. For
    repodata, this is plausible because their names include their hash.  For
    RPMs, this code includes a "mutable RPM" guard to detect files whose
    contents changed.
  - `repomd.xml` is replaced atomically (i.e.  via `rename`) after making
    available all the new RPMs & repodatas.
"""
from typing import Callable, Iterable, Iterator, Tuple

from antlir.common import get_logger, not_none
from antlir.rpm.downloader.common import DownloadConfig, DownloadResult
from antlir.rpm.downloader.repodata_downloader import gen_repodatas_from_repomds
from antlir.rpm.downloader.repomd_downloader import gen_repomds_from_repos
from antlir.rpm.downloader.rpm_downloader import gen_rpms_from_repodatas
from antlir.rpm.repo_sizer import RepoObjectVisitor
from antlir.rpm.repo_snapshot import RepoSnapshot
from antlir.rpm.yum_dnf_conf import YumDnfConfRepo


log = get_logger()


def visit_results(
    results: Iterable[DownloadResult], visitors: Iterable[RepoObjectVisitor]
) -> None:
    for res in results:
        for visitor in visitors:
            visitor.visit_repomd(res.repomd)
            # Visitors see all declared repodata, even if some downloads fail.
            for repodata in res.repomd.repodatas:
                visitor.visit_repodata(repodata)
            # Visitors inspect all RPMs, whether or not they belong to the
            # current shard.  For the RPMs in this shard, visiting after
            # downloading allows us to pass in an `Rpm` structure with
            # `.canonical_checksum` set, to better detect identical RPMs from
            # different repos.
            res_rpms = not_none(res.rpms, "rpms")
            res_sid_to_rpm = not_none(res.storage_id_to_rpm, "storage_id_to_rpm")
            for rpm in {
                **{r.location: r for r in res_rpms},
                # Post-download Rpm objects override the pre-download ones
                **{r.location: r for r in res_sid_to_rpm.values()},
            }.values():
                visitor.visit_rpm(rpm)


def download_repos(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    *,
    cfg: DownloadConfig,
    visitors: Iterable[RepoObjectVisitor] = (),
    log_sample: Callable = lambda *_, **__: None,
) -> Iterator[Tuple[YumDnfConfRepo, RepoSnapshot]]:
    all_snapshot_universes = frozenset(u for _, u in repos_and_universes)
    with cfg.new_db_ctx(readonly=False) as rw_repo_db:
        rw_repo_db.ensure_tables_exist()
        rw_repo_db.commit()

    # Concurrently download repomds, repodatas and RPMs. Materialize the
    # generators to aggregate the results between each successive step.
    repomd_results = list(gen_repomds_from_repos(repos_and_universes, cfg))
    repodata_results = list(gen_repodatas_from_repomds(repomd_results, cfg))
    rpm_results = list(
        gen_rpms_from_repodatas(
            repodata_results,
            cfg,
            all_snapshot_universes,
            log_sample=log_sample,
        )
    )

    # All downloads have completed - we now want to atomically persist repomds.
    with cfg.new_db_ctx(readonly=False) as rw_repo_db:
        # Even though a valid snapshot of a single repo is intrinsically valid,
        # we only want to operate on coherent collections of repos (as they
        # existed at roughly the same point in time). For this reason, we'd
        # rather leak already-committed repodata & RPM objects (subject to GC
        # later, if we choose) if we were not able to store a full snapshot,
        # while not doing so for repomds (as committing those essentially
        # commits a full snasphot, given that the repodata & RPM objects will
        # now be referenced).
        for res in rpm_results:
            rw_repo_db.store_repomd(res.repo_universe, res.repo.name, res.repomd)
        try:
            rw_repo_db.commit()
        except Exception:  # pragma: no cover
            # This is bad, but we hope this commit was atomic and thus none of
            # the repomds got inserted, in which case our snapshot's failed but
            # we at least don't have a semi-complete snapshot in the db.
            log.exception("Exception when trying to commit repomd")
            raise

    # Visit after having persisted all of our work
    visit_results(rpm_results, visitors)
    return (
        (
            res.repo,
            RepoSnapshot(
                repomd=res.repomd,
                storage_id_to_repodata=not_none(
                    res.storage_id_to_repodata, "storage_id_to_repodata"
                ),
                storage_id_to_rpm=not_none(res.storage_id_to_rpm, "storage_id_to_rpm"),
            ),
        )
        for res in rpm_results
    )
