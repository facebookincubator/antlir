#!/usr/bin/env python3
'''
Produces a repo-atomic snapshot of every repo in the specified `yum.conf`
(details on the atomicity guarantee in the `repo_downloader.py` docblock).

Note that there is no way to capture all repos atomically, so if e.g.  an
RPM is moved from one repo to another, it is possible for the RPM to either
occur in BOTH repos, or in NEITHER, depending on how the move is executed.
We hope that RPM moves are implemented so that the repo gaining the RPM is
re-indexed before the repo losing the RPM, because the snapshotter has no
recourse if the ground truth repo data transiently loses some RPMs.
Furthermore, multi-repo updates ought to try to swap out all the
`repomd.xml`s in as short a time as possible to minimize th chance of races.

Future: We should download the `repomd.xml` files repeatedly in the same
sequence with a short delay, until they no longer change.  Then we know that
we did not race a multi-repo update (or that the update was very slow, for
which we can never have a proper recourse) and can proceed to snapshot all
these `repomd.xml`s.  Note: if we take too long with the snapshots, it is
possible for some of the repodata or RPMs backing these `repomd.xml`s to get
deleted.  This can be mitigated e.g. by doing uncontrolled snapshots (what
we have today) across many shards, and once most of the snapshots are
up-to-date to do the 0:1 snapshot with the above `repomd.xml` checks.
'''
import argparse
import os
import sys

from io import StringIO
from typing import Iterable, List

from fs_image.common import get_file_logger, shuffled

from .common import (
    create_ro, init_logging, Path, populate_temp_dir_and_rename, retry_fn,
    RpmShard,
)
from .common_args import add_standard_args
from .gpg_keys import snapshot_gpg_keys
from .repo_db import RepoDBContext
from .repo_downloader import RepoDownloader
from .repo_sizer import RepoSizer
from .repo_snapshot import RepoSnapshot
from .storage import Storage
from .yum_dnf_conf import YumDnf, YumDnfConfParser, YumDnfConfRepo

log = get_file_logger(__file__)


def _write_confs_get_repos(
    dest: Path, yum_conf_content: str, dnf_conf_content: str,
) -> Iterable[YumDnfConfRepo]:
    yum_dnf_repos = []
    for out_name, content in [
        ('yum.conf', yum_conf_content), ('dnf.conf', dnf_conf_content),
    ]:
        if content is not None:
            with create_ro(dest / out_name, 'w') as out:
                out.write(content)
            yum_dnf_repos.append(set(
                YumDnfConfParser(YumDnf.dnf, StringIO(content)).gen_repos()
            ))
    yum_repos, dnf_repos = yum_dnf_repos
    diff_repos = yum_repos.symmetric_difference(dnf_repos)
    if diff_repos:  # pragma: no cover
        # This is not allowed because `RpmActionItem` needs the package sets
        # to be the same for `yum` or `dnf`, since it uses the
        # `snapshot.sql3` DB to validate package names and determine
        # allowable versions (aka versionlock).
        #
        # We could potentially tag every `rpm` row with "dnf" or "yum" or
        # "both" to resolve this.  In that case, the right logic would be to
        # merge the repo lists here, and to check that `yum_dnf` column in
        # any queries from the compiler.  We really don't need this extra
        # complexity today.
        raise RuntimeError(
            f'`--yum-conf` and `--dnf-conf` had different repos {diff_repos}'
        )
    return dnf_repos


def snapshot_repos(
    dest: Path, *,
    yum_conf_content: str,
    dnf_conf_content: str,
    repo_db_ctx: RepoDBContext,
    storage: Storage,
    rpm_shard: RpmShard,
    gpg_key_whitelist_dir: str,
    retries: int,
):
    declared_sizer = RepoSizer()
    saved_sizer = RepoSizer()
    repos = _write_confs_get_repos(dest, yum_conf_content, dnf_conf_content)
    os.mkdir(dest / 'repos')
    with RepoSnapshot.add_sqlite_to_storage(storage, dest) as db:
        # Randomize the order to reduce contention from concurrent writers
        for repo in shuffled(repos):
            log.info(f'Downloading repo {repo.name} from {repo.base_url}')
            with populate_temp_dir_and_rename(
                dest / 'repos' / repo.name, overwrite=True
            ) as td:
                # This is outside the retry_fn not to mask transient
                # verification failures.  I don't expect many infra failures.
                snapshot_gpg_keys(
                    key_urls=repo.gpg_key_urls,
                    whitelist_dir=gpg_key_whitelist_dir,
                    snapshot_dir=td,
                )
                retry_fn(
                    lambda: RepoDownloader(
                        repo.name, repo.base_url, repo_db_ctx, storage
                    ).download(
                        rpm_shard=rpm_shard, visitors=[declared_sizer]
                    ),
                    delays=[0] * retries,
                    what=f'Download failed: {repo.name} from {repo.base_url}',
                ).visit(saved_sizer).to_sqlite(repo.name, db)

    log.info(declared_sizer.get_report(
        f'According to their repodata, these {len(repos)} repos weigh'
    ))
    log.info(saved_sizer.get_report(f'This {rpm_shard} snapshot weighs'))


def snapshot_repos_from_args(argv: List[str]):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    add_standard_args(parser)
    parser.add_argument(
        '--dnf-conf', type=Path.from_argparse,
        help='Snapshot this `dnf.conf`, and all the repos that it lists. '
            'Can be set together with `--yum-conf`, in which case repos from '
            'both configs must be identical. At least one of these `--*-conf` '
            'options is required.',
    )
    parser.add_argument(
        '--yum-conf', type=Path.from_argparse,
        help='Snapshot this `yum.conf`; see help for `--dnf-conf`',
    )
    args = parser.parse_args(argv)

    init_logging(debug=args.debug)

    with populate_temp_dir_and_rename(args.snapshot_dir, overwrite=True) as td:
        snapshot_repos(
            dest=td,
            yum_conf_content=args.yum_conf.read_text()
                if args.yum_conf else None,
            dnf_conf_content=args.dnf_conf.read_text()
                if args.dnf_conf else None,
            repo_db_ctx=RepoDBContext(args.db, args.db.SQL_DIALECT),
            storage=args.storage,
            rpm_shard=args.rpm_shard,
            gpg_key_whitelist_dir=args.gpg_key_whitelist_dir,
            retries=args.retries,
        )


if __name__ == '__main__':  # pragma: no cover
    snapshot_repos_from_args(sys.argv[1:])
