#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
`snapshot-repo` is mostly intended for testing downloads of a single repo.
In production, you will usually want `snapshot-repos`, which will snapshot
all repos from a given `yum.conf`.
"""
import argparse
import sys

from antlir.common import get_logger, init_logging
from antlir.fs_utils import Path, populate_temp_dir_and_rename

from antlir.rpm.common_args import add_standard_args
from antlir.rpm.downloader.common import DownloadConfig
from antlir.rpm.downloader.repo_downloader import download_repos
from antlir.rpm.gpg_keys import snapshot_gpg_keys
from antlir.rpm.repo_sizer import RepoSizer
from antlir.rpm.repo_snapshot import RepoSnapshot
from antlir.rpm.storage import Storage
from antlir.rpm.yum_dnf_conf import YumDnfConfRepo


log = get_logger()


def snapshot_repo(argv) -> None:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    add_standard_args(parser)
    parser.add_argument(
        "--repo-universe",
        required=True,
        help="This is explained in the `repo_db.py` docblock. In production, "
        "it is important for the universe name to match existing "
        "conventions -- DO NOT JUST MAKE ONE UP.",
    )
    parser.add_argument(
        "--repo-name",
        required=True,
        help="Used to distinguish this repo's metadata from others' in the DB.",
    )
    parser.add_argument(
        "--repo-url",
        required=True,
        help="The base URL of the repo -- the part before repodata/repomd.xml. "
        "Supported protocols include file://, https://, and http://.",
    )
    parser.add_argument(
        "--gpg-url",
        required=True,
        action="append",
        help="(May be repeated) Yum will need to import this key to gpgcheck "
        "the repo. To avoid placing blind trust in these keys (e.g. in "
        "case this is an HTTP URL), they are verified against "
        "`--gpg-key-allowlist-dir`",
    )
    args = Path.parse_args(parser, argv)

    init_logging(debug=args.debug)

    with populate_temp_dir_and_rename(
        args.snapshot_dir,
        overwrite=True
        # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
    ) as td, RepoSnapshot.add_sqlite_to_storage(
        # pyre-fixme[6]: For 1st param expected `Storage` but got `Pluggable`.
        Storage.from_json(args.storage),
        td,
    ) as sqlite_db:
        sizer = RepoSizer()
        snapshot_gpg_keys(
            key_urls=args.gpg_url,
            allowlist_dir=args.gpg_key_allowlist_dir,
            snapshot_dir=td,
        )
        repo = YumDnfConfRepo(
            name=args.repo_name,
            base_url=args.repo_url,
            gpg_key_urls=args.gpg_url,
        )
        _, snapshot = next(
            download_repos(
                repos_and_universes=[(repo, args.repo_universe)],
                cfg=DownloadConfig(
                    db_cfg=args.db,
                    storage_cfg=args.storage,
                    rpm_shard=args.rpm_shard,
                    threads=args.threads,
                ),
            )
        )
        snapshot.visit(sizer).to_sqlite(args.repo_name, sqlite_db)
        log.info(sizer.get_report(f"This {args.rpm_shard} snapshot weighs"))


def main() -> None:  # pragma: no cover
    snapshot_repo(sys.argv[1:])


if __name__ == "__main__":
    main()  # pragma: no cover
