#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import shutil
import sqlite3
import unittest

from fs_image.fs_utils import Path, temp_dir

from . import temp_repos

from ..repo_snapshot import RepoSnapshot
from ..snapshot_repo import snapshot_repo
from ..storage import Storage


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

            storage_dict = {
                'key': 'test',
                'kind': 'filesystem',
                'base_dir': td / 'storage',
            }
            snapshot_repo([
                '--repo-universe=fakeverse',
                '--repo-name=dog',
                '--repo-url=' + (repos_root / "0/dog").file_url(),
                f'--gpg-key-whitelist-dir={whitelist_dir}',
                '--gpg-url=' + (td / 'fake_gpg_key').file_url(),
                f'--snapshot-dir={td / "snap"}',
                f'--storage={Path.json_dumps(storage_dict)}',
                '--db=' + Path.json_dumps({
                    'kind': 'sqlite',
                    'db_path': td / 'db.sqlite3',
                }),
                '--threads=4',
            ])
            # This test simply checks the overall integration, so we don't
            # bother looking inside the DB or Storage, or inspecting the
            # details of the snapshot -- those should all be covered by
            # lower-level tests.
            with sqlite3.connect(RepoSnapshot.fetch_sqlite_from_storage(
                Storage.make(**storage_dict),
                td / 'snap',
                td / 'snapshot.sql3',
            )) as db:
                self.assertEqual({
                    'dog-pkgs/rpm-test-carrot-2-rc0.x86_64.rpm',
                    'dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm',
                    'dog-pkgs/rpm-test-milk-1.41-42.x86_64.rpm',
                    'dog-pkgs/rpm-test-mutable-a-f.x86_64.rpm',
                }, {
                    path for path, in db.execute(
                        'SELECT "path" FROM "rpm";'
                    ).fetchall()
                })
