#!/usr/bin/env python3
'See `temp_snapshot` below.'
import os
import textwrap

from ..common import Path, temp_dir, RpmShard, populate_temp_dir_and_rename
from ..db_connection import DBConnectionContext
from ..repo_db import RepoDBContext, SQLDialect
from ..storage import Storage
from ..snapshot_repos import snapshot_repos
from ..tests.temp_repos import SAMPLE_STEPS, temp_repos_steps


def _make_test_yum_conf(repos_path: Path, gpg_key_path: Path) -> str:
    return textwrap.dedent('''\
        [main]
        cachedir=/var/cache/yum
        debuglevel=2
        keepcache=1
        logfile=/var/log/yum.log
        pkgpolicy=newest
        showdupesfromrepos=1
    ''') + '\n\n'.join(
        textwrap.dedent(f'''\
            [{repo}]
            baseurl={(repos_path / repo).file_url()}
            enabled=1
            name={repo}
            gpgkey={gpg_key_path.file_url()}
        ''') for repo in os.listdir(repos_path.decode()) if repo != 'yum.conf'
    )


def make_temp_snapshot(
    repos, out_dir, gpg_key_path, gpg_key_whitelist_dir,
) -> Path:
    'Generates temporary RPM repo snapshots for tests to use as inputs.'
    repo_json_dir = td / 'repos'
    os.mkdir(repo_json_dir)

    with temp_repos_steps(repo_change_steps=[repos]) as repos_root:
        snapshot_repos(
            dest=repo_json_dir,
            yum_conf_content=_make_test_yum_conf(
                # Snapshot the 0th step only, since only that is defined
                repos_root / '0', gpg_key_path,
            ),
            repo_db_ctx=RepoDBContext(
                DBConnectionContext.make(
                    kind='sqlite', db_path=(td / 'db.sqlite3').decode(),
                ),
                SQLDialect.SQLITE3,
            ),
            storage=Storage.make(
                key='test',
                kind='filesystem',
                base_dir=(td / b'storage').decode(),
            ),
            rpm_shard=RpmShard(shard=0, modulo=1),
            gpg_key_whitelist_dir=no_gpg_keys_yet,
            retries=0,  # Nothing here should require retries, it's a bug.
        )


if __name__ == '__main__':
    import argparse

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        'out_dir', help='Write the temporary snapshot to this directory.',
    )
    args = parser.parse_args()

    with temp_dir() as no_gpg_keys_yet, populate_temp_dir_and_rename(
        args.out_dir, overwrite=False,  # Buck always gives us a clean workspace
    ) as td:
        # It's a non-negligible amount of work to enable Buck to package
        # empty directories into XARs / PARs.  And, I do plan to add GPG
        # checking to the test repos.  Therefore, let's add this key
        # placeholder to make the gpg key directories non-empty.
        gpg_key_path = no_gpg_keys_yet / 'placeholder'
        open(gpg_key_path, 'a').close()
        make_temp_snapshot(SAMPLE_STEPS[0], td, gpg_key_path, no_gpg_keys_yet)
