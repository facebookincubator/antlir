#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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
import json
import os
import sys

from configparser import ConfigParser
from io import StringIO
from typing import Callable, Dict, FrozenSet, Iterable, List

from fs_image.common import get_file_logger, init_logging
from fs_image.fs_utils import create_ro, Path, populate_temp_dir_and_rename
from fs_image.rpm.downloader.common import DownloadConfig
from fs_image.rpm.downloader.repo_downloader import download_repos

from .common import RpmShard
from .common_args import add_standard_args
from .gpg_keys import snapshot_gpg_keys
from .repo_db import validate_universe_name
from .repo_sizer import RepoSizer
from .repo_snapshot import RepoSnapshot
from .storage import Storage
from .yum_dnf_conf import YumDnf, YumDnfConfParser, YumDnfConfRepo

log = get_file_logger(__file__)


def _write_confs_get_repos(
    dest: Path,
    yum_conf_content: str,
    dnf_conf_content: str,
    *,
    exclude_repos: FrozenSet[str],
) -> Iterable[YumDnfConfRepo]:
    assert not (exclude_repos & {'main', 'DEFAULT'}), exclude_repos
    yum_dnf_repos = []
    for out_name, content in [
        ('yum.conf', yum_conf_content), ('dnf.conf', dnf_conf_content),
    ]:
        if content is not None:
            # Save the original, unmodified config in case of an error
            with create_ro(dest / (out_name + '.orig'), 'w') as out:
                out.write(content)
            # Remove the excluded repos
            cp = ConfigParser()
            cp.read_string(content)
            for excluded in exclude_repos:
                cp.remove_section(excluded)
            with create_ro(dest / out_name, 'w+') as out:
                cp.write(out)
                out.seek(0)
                new_content = out.read()
            yum_dnf_repos.append(set(
                YumDnfConfParser(YumDnf.dnf, StringIO(new_content)).gen_repos()
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
    repo_to_universe: Callable[[YumDnfConfRepo], str],
    yum_conf_content: str,
    dnf_conf_content: str,
    db_cfg: Dict[str, str],
    storage_cfg: Dict[str, str],
    rpm_shard: RpmShard,
    gpg_key_whitelist_dir: str,
    exclude: FrozenSet[str],
    threads: int,
):
    all_repos_sizer = RepoSizer()
    shard_sizer = RepoSizer()
    repos = _write_confs_get_repos(
        dest, yum_conf_content, dnf_conf_content, exclude_repos=exclude
    )
    os.mkdir(dest / 'repos')
    repos_and_universes = [
        # Evaluated eagerly for `all_snapshot_universes`.  Bonus: this also
        # fails fast if some repos cannot be resolved.
        (repo, validate_universe_name(repo_to_universe(repo)))
            for repo in repos
            if repo.name not in exclude
    ]
    with RepoSnapshot.add_sqlite_to_storage(
        Storage.from_json(storage_cfg), dest
    ) as db:
        for repo, snapshot in download_repos(
            repos_and_universes=repos_and_universes,
            cfg=DownloadConfig(
                db_cfg=db_cfg,
                storage_cfg=storage_cfg,
                rpm_shard=rpm_shard,
                threads=threads,
            ),
            visitors=[all_repos_sizer],
        ):
            log.debug(f"repo.name: {repo.name}, repo.base_url: {repo.base_url}")
            snapshot.visit(shard_sizer).to_sqlite(repo.name, db)

            log.debug(f'repo.gpg_key_urls: {repo.gpg_key_urls}')
            # This is done outside of the repo snapshot as we only want to
            # perform it upon successful snapshot. It's also a quick operation
            # and thus doesn't benefit from the added complexity of threading
            with populate_temp_dir_and_rename(
                dest / 'repos' / repo.name, overwrite=True
            ) as td:
                snapshot_gpg_keys(
                    key_urls=repo.gpg_key_urls,
                    whitelist_dir=gpg_key_whitelist_dir,
                    snapshot_dir=td,
                )

    log.info(all_repos_sizer.get_report(
        f'According to their repodata, these {len(repos)} repos weigh'
    ))
    log.info(shard_sizer.get_report(f'This {rpm_shard} snapshot weighs'))


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
    parser.add_argument(
        '--exclude', action='append', default=[],
        help='Repos to be excluded in the snapshot.',
    )

    universe_warning = (
        'Described in the `repo_db.py` docblock. In production, it is '
        'important for the universe name to match existing conventions -- '
        'DO NOT JUST MAKE ONE UP.'
    )
    universe_group = parser.add_mutually_exclusive_group(required=True)
    universe_group.add_argument(
        '--repo-to-universe-json', type=Path.from_argparse,
        help='JSON dict of repo name to universe name. ' + universe_warning,
    )
    universe_group.add_argument(
        '--one-universe-for-all-repos',
        help='Snapshot all repos under this universe name. ' + universe_warning,
    )

    args = Path.parse_args(parser, argv)

    init_logging(debug=args.debug)

    if args.one_universe_for_all_repos:
        def repo_to_universe(_repo):
            return args.one_universe_for_all_repos
    elif args.repo_to_universe_json:
        with open(args.repo_to_universe_json) as ru_json:
            repo_to_universe_json = json.load(ru_json)

        def repo_to_universe(repo):
            return repo_to_universe_json[repo.name]
    else:  # pragma: no cover
        raise AssertionError(args)

    with populate_temp_dir_and_rename(args.snapshot_dir, overwrite=True) as td:
        snapshot_repos(
            dest=td,
            repo_to_universe=repo_to_universe,
            yum_conf_content=args.yum_conf.read_text()
                if args.yum_conf else None,
            dnf_conf_content=args.dnf_conf.read_text()
                if args.dnf_conf else None,
            db_cfg=args.db,
            storage_cfg=args.storage,
            rpm_shard=args.rpm_shard,
            gpg_key_whitelist_dir=args.gpg_key_whitelist_dir,
            exclude=frozenset(args.exclude),
            threads=args.threads,
        )


if __name__ == '__main__':  # pragma: no cover
    snapshot_repos_from_args(sys.argv[1:])
