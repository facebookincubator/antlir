#!/usr/bin/env python3
'''
`snapshot-repo` is mostly intended for testing downloads of a single repo.
In production, you will usually want `snapshot-repos`, which will snapshot
all repos from a given `yum.conf`.
'''
import argparse
import sys

from fs_image.rpm.downloader.common import DownloadConfig
from fs_image.rpm.downloader.repo_downloader import download_repos
from .common import get_file_logger, init_logging, populate_temp_dir_and_rename
from .common_args import add_standard_args
from .repo_sizer import RepoSizer
from .repo_snapshot import RepoSnapshot
from .storage import Storage
from .gpg_keys import snapshot_gpg_keys
from .yum_dnf_conf import YumDnfConfRepo

log = get_file_logger(__file__)


def snapshot_repo(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    add_standard_args(parser)
    parser.add_argument(
        '--repo-universe', required=True,
        help='This is explained in the `repo_db.py` docblock. In production, '
            'it is important for the universe name to match existing '
            'conventions -- DO NOT JUST MAKE ONE UP.',
    )
    parser.add_argument(
        '--repo-name', required=True,
        help="Used to distinguish this repo's metadata from others' in the DB.",
    )
    parser.add_argument(
        '--repo-url', required=True,
        help='The base URL of the repo -- the part before repodata/repomd.xml. '
            'Supported protocols include file://, https://, and http://.',
    )
    parser.add_argument(
        '--gpg-url', required=True, action='append',
        help='(May be repeated) Yum will need to import this key to gpgcheck '
            'the repo. To avoid placing blind trust in these keys (e.g. in '
            'case this is an HTTP URL), they are verified against '
            '`--gpg-key-whitelist-dir`',
    )
    args = parser.parse_args(argv)

    init_logging(debug=args.debug)

    with populate_temp_dir_and_rename(
        args.snapshot_dir, overwrite=True,
    ) as td, RepoSnapshot.add_sqlite_to_storage(
        Storage.from_json(args.storage), td
    ) as sqlite_db:
        sizer = RepoSizer()
        snapshot_gpg_keys(
            key_urls=args.gpg_url,
            whitelist_dir=args.gpg_key_whitelist_dir,
            snapshot_dir=td,
        )
        repo = YumDnfConfRepo(
            name=args.repo_name,
            base_url=args.repo_url,
            gpg_key_urls=args.gpg_url
        )
        _, snapshot = next(download_repos(
            repos_and_universes=[(repo, args.repo_universe)],
            cfg=DownloadConfig(
                db_cfg=args.db,
                storage_cfg=args.storage,
                rpm_shard=args.rpm_shard,
                threads=args.threads,
            ),
        ))
        snapshot.visit(sizer).to_sqlite(args.repo_name, sqlite_db)
        log.info(sizer.get_report(f'This {args.rpm_shard} snapshot weighs'))


if __name__ == '__main__':  # pragma: no cover
    snapshot_repo(sys.argv[1:])
