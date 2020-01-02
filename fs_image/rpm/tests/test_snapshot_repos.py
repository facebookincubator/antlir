#!/usr/bin/env python3
import json
import sqlite3
import unittest
import unittest.mock
import tempfile

from . import temp_repos

from ..common import temp_dir

from .. import repo_db
from ..repo_snapshot import RepoSnapshot
from ..snapshot_repos import snapshot_repos_from_args
from ..storage import Storage


class SnapshotReposTestCase(unittest.TestCase):

    def setUp(self):
        self.maxDiff = 12345

    def test_snapshot(self):
        with temp_repos.temp_repos_steps(repo_change_steps=[
            {  # All of the `snap0` repos are in the "mammal" universe
                'bunny': temp_repos.SAMPLE_STEPS[0]['bunny'],
                'cat': temp_repos.SAMPLE_STEPS[0]['cat'],
                'dog': temp_repos.SAMPLE_STEPS[0]['dog'],
                'kitteh': 'cat',
            },
            {  # Some of these are "zombie"s, see `ru_json` below.
                # 'bunny' stays unchanged, with the same `repomd.xml`
                'cat': temp_repos.SAMPLE_STEPS[1]['cat'],
                'dog': temp_repos.SAMPLE_STEPS[1]['dog'],
                # 'kitteh' stays unchanged, with the same `repomd.xml`
            },
        ]) as repos_root, temp_dir() as td:
            storage_dict = {
                'key': 'test',
                'kind': 'filesystem',
                'base_dir': (td / 'storage').decode(),
            }
            repo_db_path = td / 'db.sqlite3'

            # Mock all repomd fetch timestamps to be identical to test that
            # multiple universes do not collide.
            orig_store_repomd = repo_db.RepoDBContext.store_repomd
            with unittest.mock.patch.object(
                repo_db.RepoDBContext, 'store_repomd',
                lambda self, universe_s, repo_s, repomd:
                    orig_store_repomd(
                        self, universe_s, repo_s, repomd._replace(
                            fetch_timestamp=451,
                        ),
                    )
            ), tempfile.NamedTemporaryFile('w') as ru_json:
                common_args = [
                    '--gpg-key-whitelist-dir', (td / 'gpg_whitelist').decode(),
                    '--storage', json.dumps(storage_dict),
                    '--db', json.dumps({
                        'kind': 'sqlite',
                        'db_path': repo_db_path.decode(),
                    }),
                ]
                snapshot_repos_from_args(common_args + [
                    '--one-univese-for-all-repos', 'mammal',
                    '--dnf-conf', (repos_root / '0/dnf.conf').decode(),
                    '--yum-conf', (repos_root / '0/yum.conf').decode(),
                    '--snapshot-dir', (td / 'snap0').decode(),
                ])
                json.dump({
                    'bunny': 'mammal',  # Same content as in snap0
                    'cat': 'zombie',  # Changes content from snap0
                    'dog': 'mammal',  # Changes content from snap0
                    'kitteh': 'zombie',  # Same content as in snap0
                }, ru_json)
                ru_json.flush()
                snapshot_repos_from_args(common_args + [
                    '--repo-to-universe-json', ru_json.name,
                    '--dnf-conf', (repos_root / '1/dnf.conf').decode(),
                    '--yum-conf', (repos_root / '1/yum.conf').decode(),
                    '--snapshot-dir', (td / 'snap1').decode(),
                ])

            with sqlite3.connect(repo_db_path) as db:
                # Check that repomd rows are repeated or duplicated as we'd
                # expect across `snap[01]`, and the universes.
                repo_mds = sorted(db.execute('''
                    SELECT "universe", "repo", "fetch_timestamp", "checksum"
                    FROM "repo_metadata"
                ''').fetchall())
                self.assertEqual([
                    (b'mammal', b'bunny', 451),  # both snap0 and snap1
                    (b'mammal', b'cat', 451),  # snap0
                    # There are two different `repomd`s in snap0 and snap1
                    (b'mammal', b'dog', 451),
                    (b'mammal', b'dog', 451),
                    (b'mammal', b'kitteh', 451),  # snap0 -- index -3
                    (b'zombie', b'cat', 451),  # snap1
                    (b'zombie', b'kitteh', 451),  # snap1 -- index -1
                ], [r[:3] for r in repo_mds])
                # The kittehs have the same checksums, but exist separately
                # due to being in different universes.
                self.assertEqual(repo_mds[-1][1:], repo_mds[-3][1:])

                # We expect this identical "carrot" RPM (same checksums) to
                # be repeated because it occurs in two different universes.
                kitteh_carrot_filename = b'rpm-test-carrot-1-lockme.x86_64.rpm'
                kitteh_carrots = sorted(db.execute('''
                    SELECT "universe", "filename", "checksum" FROM "rpm"
                    WHERE "filename" == ?
                ''', [kitteh_carrot_filename]).fetchall())
                kitteh_carrot_chksum = kitteh_carrots[0][2]
                self.assertEqual([
                    (b'mammal', kitteh_carrot_filename, kitteh_carrot_chksum),
                    (b'zombie', kitteh_carrot_filename, kitteh_carrot_chksum),
                ], kitteh_carrots)

                # This RPM has two variants for its contents at step 1.
                # This creates a mutable RPM error in `snap1`.
                milk2_filename = b'rpm-test-milk-2.71-8.x86_64.rpm'
                milk2s = sorted(db.execute('''
                    SELECT "universe", "filename", "checksum" FROM "rpm"
                    WHERE "filename" == ?
                ''', [milk2_filename]).fetchall())
                milk2_chksum_step0 = milk2s[0][2]  # mammal sorts first
                milk2_chksum_step1, = {milk2s[1][2], milk2s[2][2]} - {
                    milk2_chksum_step0
                }
                self.assertEqual(sorted([
                    # snap0 cat & kitteh
                    (b'mammal', milk2_filename, milk2_chksum_step0),
                    # snap1 kitteh -- mutable RPM error vs "snap1 cat"
                    (b'zombie', milk2_filename, milk2_chksum_step0),
                    # snap1 cat -- mutable RPM error vs "snap1 kitteh"
                    (b'zombie', milk2_filename, milk2_chksum_step1),
                ]), milk2s)

                # This RPM changes contents between step 0 and step 1, but
                # since they land in different universes, there is no
                # mutable RPM error.
                mutable_filename = b'rpm-test-mutable-a-f.x86_64.rpm'
                mutables = sorted(db.execute('''
                    SELECT "universe", "filename", "checksum" FROM "rpm"
                    WHERE "filename" == ?
                ''', [mutable_filename]).fetchall())
                mutable_chksum_dog = mutables[0][2]  # mammal sorts first
                mutable_chksum_cat = mutables[1][2]
                self.assertEqual(sorted([
                    # snap0 dog
                    (b'mammal', mutable_filename, mutable_chksum_dog),
                    # snap1 cat
                    (b'zombie', mutable_filename, mutable_chksum_cat),
                ]), mutables)

            # As with `test_snapshot_repo`, this is not a complete check of
            # the snapshot state.  We only check for sanity, and for the
            # interactions between multiple snapshots & multiple universes.
            # Lower-level tests check many other lower-level details.
            mutable_a_f_checksums = set()
            milk2_checksums = set()
            expected_errors = 1
            for snap_name, expected_rows in [
                # These are just straight up "bunny", "cat" (with alias),
                # and "dog" from SAMPLE_STEPS[0], as indicated in our setup.
                ('snap0', {
                    ('bunny', 'bunny-pkgs/rpm-test-carrot-2-rc0'),
                    ('cat', 'cat-pkgs/rpm-test-carrot-1-lockme'),
                    ('cat', 'cat-pkgs/rpm-test-mice-0.1-a'),
                    ('cat', 'cat-pkgs/rpm-test-milk-2.71-8'),
                    ('dog', 'dog-pkgs/rpm-test-milk-1.41-42'),
                    ('dog', 'dog-pkgs/rpm-test-carrot-2-rc0'),
                    ('dog', 'dog-pkgs/rpm-test-mice-0.1-a'),
                    ('dog', 'dog-pkgs/rpm-test-mutable-a-f'),
                    ('kitteh', 'cat-pkgs/rpm-test-carrot-1-lockme'),
                    ('kitteh', 'cat-pkgs/rpm-test-mice-0.1-a'),
                    ('kitteh', 'cat-pkgs/rpm-test-milk-2.71-8'),
                }),
                # These are "bunny" & "cat" (as "kitteh") from
                # SAMPLE_STEPS[0], plus "cat" & "dog from SAMPLE_STEPS[1].
                #
                ('snap1', {
                    ('bunny', 'bunny-pkgs/rpm-test-carrot-2-rc0'),
                    ('cat', 'cat-pkgs/rpm-test-milk-2.71-8'),  # may error
                    ('cat', 'cat-pkgs/rpm-test-mice-0.2-rc0'),
                    # We'd have gotten a "mutable RPM" error if this
                    # were in the same universe as the "mutable" from
                    # "dog" in snap0.
                    ('cat', 'cat-pkgs/rpm-test-mutable-a-f'),
                    ('dog', 'dog-pkgs/rpm-test-carrot-2-rc0'),
                    ('dog', 'dog-pkgs/rpm-test-bone-5i-beef'),
                    ('kitteh', 'cat-pkgs/rpm-test-carrot-1-lockme'),
                    ('kitteh', 'cat-pkgs/rpm-test-mice-0.1-a'),
                    ('kitteh', 'cat-pkgs/rpm-test-milk-2.71-8'),  # may error
                }),
            ]:
                with sqlite3.connect(RepoSnapshot.fetch_sqlite_from_storage(
                    Storage.make(**storage_dict),
                    td / snap_name,
                    td / snap_name / 'snapshot.sql3',
                )) as db:
                    rows = db.execute(
                        'SELECT "repo", "path", "error", "checksum" FROM "rpm"'
                    ).fetchall()
                    self.assertEqual({
                        (r, p + '.x86_64.rpm') for r, p in expected_rows
                    }, {
                        (r, p) for r, p, _e, _c in rows
                    })
                    for repo, path, error, chksum in rows:
                        # There is just 1 error among all the rows.  The
                        # "milk-2.71" RPM from either "kitteh" or "cat" in
                        # `snap1` gets marked with "mutable_rpm".  Which
                        # repo gets picked depends on the (shuffled) order
                        # of the snapshot.  If we were to run the `snap1`
                        # snapshot a second time, both would get marked.
                        if error is not None:
                            expected_errors -= 1
                            self.assertEqual((
                                'snap1',
                                'cat-pkgs/rpm-test-milk-2.71-8.x86_64.rpm',
                                'mutable_rpm',
                            ), (snap_name, path, error), repo)
                            self.assertIn(repo, {'cat', 'kitteh'})
                        # Sanity-check checksums
                        self.assertTrue(chksum.startswith('sha384:'), chksum)
                        if path == 'cat-pkgs/rpm-test-milk-2.71-8.x86_64.rpm':
                            milk2_checksums.add(chksum)
                        if path.endswith('rpm-test-mutable-a-f.x86_64.rpm'):
                            mutable_a_f_checksums.add(chksum)

            self.assertEqual(0, expected_errors)
            self.assertEqual(2, len(milk2_checksums))
            self.assertEqual(2, len(mutable_a_f_checksums))
