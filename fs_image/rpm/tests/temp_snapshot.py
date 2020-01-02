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


def _make_test_yum_dnf_conf(
    yum_dnf: str, repos_path: Path, gpg_key_path: Path,
) -> str:
    return textwrap.dedent(f'''\
        [main]
        cachedir=/var/cache/{yum_dnf}
        debuglevel=2
        keepcache=1
        logfile=/var/log/{yum_dnf}.log
        pkgpolicy=newest
        showdupesfromrepos=1
    ''') + '\n\n'.join(
        textwrap.dedent(f'''\
            [{repo}]
            baseurl={(repos_path / repo).file_url()}
            enabled=1
            name={repo}
            gpgkey={gpg_key_path.file_url()}
        ''') for repo in os.listdir(repos_path.decode())
            if repo not in ('dnf.conf', 'yum.conf')
    )


def make_temp_snapshot(
    repos, out_dir, gpg_key_path, gpg_key_whitelist_dir,
) -> Path:
    'Generates temporary RPM repo snapshots for tests to use as inputs.'
    snapshot_dir = out_dir / 'temp_snapshot_dir'
    os.mkdir(snapshot_dir)

    with temp_repos_steps(repo_change_steps=[repos]) as repos_root:
        snapshot_repos(
            dest=snapshot_dir,
            # `SnapshotReposTestCase` covers multi-universe handling
            repo_to_universe=lambda _repo: 'generic',
            # Snapshot the 0th step only, since only that is defined
            yum_conf_content=_make_test_yum_dnf_conf(
                'yum', repos_root / '0', gpg_key_path,
            ),
            dnf_conf_content=_make_test_yum_dnf_conf(
                'dnf', repos_root / '0', gpg_key_path,
            ),
            repo_db_ctx=RepoDBContext(
                DBConnectionContext.make(
                    kind='sqlite', db_path=(out_dir / 'db.sqlite3').decode(),
                ),
                SQLDialect.SQLITE3,
            ),
            storage=Storage.make(
                key='test',
                kind='filesystem',
                base_dir=(out_dir / 'storage').decode(),
            ),
            rpm_shard=RpmShard(shard=0, modulo=1),
            gpg_key_whitelist_dir=no_gpg_keys_yet,
            retries=0,  # Nothing here should require retries, it's a bug.
        )

    # Merge the repo snapshot with the storage & RPM DB -- this makes our
    # test snapshot build target look very much like prod snapshots.
    for f in os.listdir(snapshot_dir):
        assert not os.path.exists(out_dir / f), f'Must not overwrite {f}'
        os.rename(snapshot_dir / f, out_dir / f)
    os.rmdir(snapshot_dir)

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
