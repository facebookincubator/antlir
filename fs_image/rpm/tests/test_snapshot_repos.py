#!/usr/bin/env python3
import json
import sqlite3
import unittest
import tempfile

from . import temp_repos

from ..common import temp_dir

from ..repo_snapshot import RepoSnapshot
from ..snapshot_repos import snapshot_repos_from_args
from ..storage import Storage


class SnapshotReposTestCase(unittest.TestCase):

    def test_snapshot(self):
        with temp_repos.temp_repos_steps(repo_change_steps=[{
            'cat': temp_repos.SAMPLE_STEPS[0]['cat'],
            'dog': temp_repos.SAMPLE_STEPS[0]['dog'],
        }]) as repos_root, temp_dir() as td:
            storage_dict = {
                'key': 'test',
                'kind': 'filesystem',
                'base_dir': (td / 'storage').decode(),
            }
            snapshot_repos_from_args([
                '--yum-conf', (repos_root / '0/yum.conf').decode(),
                '--gpg-key-whitelist-dir', (td / 'gpg_whitelist').decode(),
                '--snapshot-dir', (td / 'snap').decode(),
                '--storage', json.dumps(storage_dict),
                '--db', json.dumps({
                    'kind': 'sqlite',
                    'db_path': (td / 'db.sqlite3').decode(),
                }),
            ])
            # As with `test_snapshot_repo`, this is just a sanity check --
            # the lower-level details are checked by lower-level tests.
            with sqlite3.connect(RepoSnapshot.fetch_sqlite_from_storage(
                Storage.make(**storage_dict),
                td / 'snap',
                td / 'snapshot.sql3',
            )) as db:
                self.assertEqual({
                    'cat-pkgs/rpm-test-mice-0.1-a.x86_64.rpm',
                    'cat-pkgs/rpm-test-milk-2.71-8.x86_64.rpm',
                    'dog-pkgs/rpm-test-carrot-2-rc0.x86_64.rpm',
                    'dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm',
                    'dog-pkgs/rpm-test-milk-1.41-42.x86_64.rpm',
                }, {
                    path for path, in db.execute('''
                        SELECT "path" FROM "rpm"
                        WHERE "repo" in ("cat", "dog")
                        ''').fetchall()
                })
