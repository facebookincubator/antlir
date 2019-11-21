#!/usr/bin/env python3
import json
import os
import shutil
import unittest
import tempfile

from . import temp_repos

from ..common import temp_dir
from ..snapshot_repo import snapshot_repo


class SnapshotRepoTestCase(unittest.TestCase):

    def test_snapshot(self):
        with temp_repos.temp_repos_steps(repo_change_steps=[{
            'dog': temp_repos.SAMPLE_STEPS[0]['dog'],
        }]) as repos_root, temp_dir() as td:
            with open(td / 'fake_gpg_key', 'w'):
                pass

            whitelist_dir = td / 'gpg_whitelist'
            os.mkdir(whitelist_dir)
            shutil.copy(td / 'fake_gpg_key', whitelist_dir)

            snapshot_repo([
                '--repo-name', 'dog',
                '--repo-url', (repos_root / '0/dog').file_url(),
                '--gpg-key-whitelist-dir', whitelist_dir.decode(),
                '--gpg-url', (td / 'fake_gpg_key').file_url(),
                '--snapshot-dir', (td / 'snap').decode(),
                '--storage', json.dumps({
                    'key': 'test',
                    'kind': 'filesystem',
                    'base_dir': (td / 'storage').decode(),
                }),
                '--db', json.dumps({
                    'kind': 'sqlite',
                    'db_path': (td / 'db.sqlite3').decode(),
                }),
            ])
            # This test simply checks the overall integration, so we don't
            # bother looking inside the DB or Storage, or inspecting the
            # details of the snapshot -- those should all be covered by
            # lower-level tests.
            with open(td / 'snap/rpm.json') as rpm_path:
                self.assertEqual({
                    'dog-pkgs/rpm-test-carrot-2-rc0.x86_64.rpm',
                    'dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm',
                    'dog-pkgs/rpm-test-milk-1.41-42.x86_64.rpm',
                }, set(json.load(rpm_path).keys()))
