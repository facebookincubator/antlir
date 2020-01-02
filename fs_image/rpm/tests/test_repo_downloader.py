#!/usr/bin/env python3
'''
Future work:

  - We should assert how the DB changes as a consequence of our writes.
    Right now, the coverage for this is a bit slim.  First, we cover the
    basics in `test_repo_db.py`.  Second, some of these multi-download tests
    below would fail if we did something totally broken to the DB.

  - Explicitly assert that we clean up unneeded storage IDs. Right now,
    this is implicitly asserted by having 100% code coverage -- e.g.
    `test_lose_repodata_commit_race` covers "Deleting uncommitted blobs."
'''
import os
import re
import requests
import unittest
import tempfile

from contextlib import contextmanager
from io import BytesIO
from typing import List
from unittest import mock

from fs_image.common import set_new_key

from . import temp_repos

from .. import repo_downloader

from ..common import RpmShard
from ..storage import Storage
from ..db_connection import DBConnectionContext
from ..repo_db import RepoDBContext, SQLDialect
from ..repo_snapshot import (
    FileIntegrityError, HTTPError, MutableRpmError, RepoSnapshot,
)
from ..tests.temp_repos import temp_repos_steps


def raise_fake_http_error(_contents):

    class FakeResponse:
        pass

    response = FakeResponse()
    response.status_code = 404
    raise requests.exceptions.HTTPError(response=response)


def _location_basename(rpm):
    return rpm._replace(location=os.path.basename(rpm.location))


MICE_01_RPM_REGEX = r'.*/rpm-test-mice-0\.1-a\.x86_64\.rpm$'
FILELISTS_REPODATA_REGEX = r'repodata/[0-9a-f]*-filelists\.xml\.gz$'

_GOOD_DOG = temp_repos.Repo([
    # Copy-pasta'd from `temp_repos.py` to avoid unnecessary cross-dependencies.
    temp_repos.Rpm('milk', '1.41', '42'),
    temp_repos.Rpm('mice', '0.1', 'a'),
    temp_repos.Rpm('carrot', '2', 'rc0'),
])
_GOOD_DOG_LOCATIONS = _GOOD_DOG.locations('good_dog')

# These next 2 exist to test updates to repos, and using multiple repos.
_SAUSAGE_3_BETA = temp_repos.Rpm('sausage', '3', 'beta')
_GOOD_DOG2 = _GOOD_DOG._replace(
    # Remove "carrot", add "sausage".
    rpms=_GOOD_DOG.rpms[:-1] + [_SAUSAGE_3_BETA],
)
_CHAOS_CAT = temp_repos.Repo([_SAUSAGE_3_BETA])  # Test cross-repo duplicates.
# Older "sausage" than GOOD_DOG2, treated just as any other non-duplicate RPM.
_CHAOS_CAT2 = temp_repos.Repo([temp_repos.Rpm('sausage', '3', 'alpha')])

# Tests "mutable RPM" errors
_BAD_DOG = temp_repos.Repo([
    temp_repos.Rpm(
        'milk', '1.41', '42', override_contents='differs from good_dog',
    ),
])


class RepoDownloaderTestCase(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        # Since we only read the repo, it is much faster to create it once
        # for all the tests (~4x speed-up as of writing).
        cls.temp_repos_ctx = temp_repos_steps(repo_change_steps=[
            {
                'good_dog': _GOOD_DOG,
                'chaos_cat': _CHAOS_CAT,
                'bad_dog': _BAD_DOG,
            },
            {
                'good_dog': _GOOD_DOG2,
                'chaos_cat': _CHAOS_CAT2,
            },
        ])
        cls.repos_root = cls.temp_repos_ctx.__enter__()

    @classmethod
    def tearDownClass(cls):
        cls.temp_repos_ctx.__exit__(None, None, None)

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def _make_db_context(self):
        return RepoDBContext(
            DBConnectionContext.make(kind='sqlite', db_path=':memory:'),
            SQLDialect.SQLITE3,
        )

    def _make_downloader(self, storage_dir, step_and_repo, db_context=None):
        return repo_downloader.RepoDownloader(
            repo_universe='fakeverse',
            repo_name=step_and_repo,
            repo_url=(self.repos_root / step_and_repo).file_url(),
            repo_db_ctx=self._make_db_context()
                if db_context is None else db_context,
            storage=Storage.make(
                key='test', kind='filesystem', base_dir=storage_dir,
            ),
        )

    def _check_storage_id_error(self, storage_id_to_obj, error_cls):
        'Ensure exactly one of the objects has an "error" storage ID.'
        error_dict = None
        for sid, obj in storage_id_to_obj.items():
            if isinstance(sid, str):
                continue
            self.assertIsInstance(sid, error_cls)
            self.assertIsNone(error_dict)
            error_dict = dict(sid.args)
            self.assertEqual(obj.location, error_dict['location'])
        self.assertIsNotNone(error_dict)
        return error_dict

    def _check_no_other_fields(self, snapshot, fields):
        checked_fields = {field: None for field in fields}
        self.assertEqual(
            RepoSnapshot(**checked_fields),
            snapshot._replace(**checked_fields),
        )

    def _check_snapshot(self, snapshot, rpm_locations, *, has_errors=False):
        # Repomd agrees with repodatas
        self.assertEqual(
            len(snapshot.repomd.repodatas), len(snapshot.storage_id_to_repodata)
        )
        # More repodatas exist than just the primary, but I don't want to know.
        self.assertGreaterEqual(len(snapshot.storage_id_to_repodata), 1)
        # Check that rpms are as expected,
        self.assertEqual(
            set(rpm_locations),
            {r.location for r in snapshot.storage_id_to_rpm.values()},
        )

        # If other fields get added, this reminds us to update the above test
        self._check_no_other_fields(
            snapshot, ['repomd', 'storage_id_to_repodata', 'storage_id_to_rpm'],
        )

        if not has_errors:
            for sito in [
                snapshot.storage_id_to_rpm, snapshot.storage_id_to_repodata,
            ]:
                self.assertTrue(all(isinstance(s, str) for s in sito))

    def _check_repomd_equal(self, a, b):
        self.assertEqual(
            a._replace(fetch_timestamp=None), b._replace(fetch_timestamp=None),
        )

    @contextmanager
    def _check_download_error(self, url_regex, corrupt_file_fn, error_cls):
        original_open_url = repo_downloader.open_url

        def my_open_url(url):
            if re.match(url_regex, url):
                with original_open_url(url) as f:
                    return BytesIO(corrupt_file_fn(f.read()))
            return original_open_url(url)

        with tempfile.TemporaryDirectory() as storage_dir:
            downloader = self._make_downloader(storage_dir, '0/good_dog')
            with mock.patch.object(repo_downloader, 'open_url') as mock_fn:
                mock_fn.side_effect = my_open_url
                bad_snapshot = downloader.download()
                self._check_snapshot(
                    bad_snapshot, _GOOD_DOG_LOCATIONS, has_errors=True,
                )
                # Exactly one of RPMs & repodatas will have an error.
                storage_id_to_obj, = [
                    sito for sito in [
                        bad_snapshot.storage_id_to_rpm,
                        bad_snapshot.storage_id_to_repodata,
                    ] if any(not isinstance(sid, str) for sid in sito)
                ]
                yield self._check_storage_id_error(storage_id_to_obj, error_cls)

            # Re-downloading outside of the mock results in the same
            # snapshot, but with the error corrected.
            good_snapshot = downloader.download()
            self._check_snapshot(good_snapshot, _GOOD_DOG_LOCATIONS)
            self._check_repomd_equal(good_snapshot.repomd, bad_snapshot.repomd)
            for good_sito, bad_sito in [
                (
                    good_snapshot.storage_id_to_rpm,
                    bad_snapshot.storage_id_to_rpm,
                ),
                (
                    good_snapshot.storage_id_to_repodata,
                    bad_snapshot.storage_id_to_repodata,
                ),
            ]:
                # Compare the bad snapshot with the good.
                self.assertEqual(len(good_sito), len(bad_sito))
                for bad_sid, bad_obj in bad_sito.items():
                    if isinstance(bad_sid, str):
                        # By reusing the bad snapshot's storage ID, we
                        # implicitly check that the DB prevented double-
                        # storage of the objects.
                        self.assertEqual(bad_obj, good_sito[bad_sid])
                    # else:
                    #     We can't compare much since it's annoying to find
                    #     the corresponding object in `good_sido`.

    def test_rpm_download_errors(self):
        mice_location = 'good_dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm'

        extra_bytes = b'change size'
        with self._check_download_error(
            MICE_01_RPM_REGEX, lambda s: s + extra_bytes, FileIntegrityError,
        ) as error_dict:
            self.assertEqual(mice_location, error_dict['location'])
            self.assertEqual('size', error_dict['failed_check'])
            self.assertEqual(
                len(extra_bytes),
                int(error_dict['actual']) - int(error_dict['expected']),
            )

        with self._check_download_error(
            MICE_01_RPM_REGEX,
            lambda s: b'dog' + s[3:],  # change contents
            FileIntegrityError,
        ) as error_dict:
            self.assertEqual(mice_location, error_dict['location'])
            self.assertEqual('sha256', error_dict['failed_check'])

        with self._check_download_error(
            MICE_01_RPM_REGEX, raise_fake_http_error, HTTPError,
        ) as error_dict:
            self.assertEqual(mice_location, error_dict['location'])
            self.assertEqual(404, error_dict['http_status'])

    def test_repodata_download_errors(self):
        # These are not reported as "storage IDs" because a failure to parse
        # the primary repodata is fatal -- we'd have no list of RPMs.
        with self.assertRaises(repo_downloader.RepodataParseError):
            with self._check_download_error(
                r'.*/good_dog/repodata/[0-9a-f]*-primary\.sqlite\.bz2$',
                lambda s: s[3:],  # change contents
                FileIntegrityError,
            ):
                pass

        # Since RepodataParseError is not a ReportableError, this is a
        # different code path:
        with self.assertRaises(HTTPError):
            with self._check_download_error(
                r'.*/good_dog/repodata/[0-9a-f]*-primary\.sqlite\.bz2$',
                raise_fake_http_error,
                HTTPError,
            ):
                pass

        # Failure to get non-primary repodata does not abort a snapshot.
        with self._check_download_error(
            r'.*/good_dog/' + FILELISTS_REPODATA_REGEX,
            lambda s: s[3:],  # change size
            FileIntegrityError,
        ) as error_dict:
            self.assertRegex(error_dict['location'], FILELISTS_REPODATA_REGEX)
            self.assertEqual('size', error_dict['failed_check'])
            self.assertEqual(
                -3, int(error_dict['actual']) - int(error_dict['expected']),
            )

    def _download_repo_twice(self, storage_dir, repo, step_and_repo, db_ctx):
        downloader = self._make_downloader(storage_dir, step_and_repo, db_ctx)
        snap1 = downloader.download()
        snap2 = downloader.download()
        self._check_repomd_equal(snap1.repomd, snap2.repomd)
        self.assertEqual(
            snap1._replace(repomd=None), snap2._replace(repomd=None),
        )
        self._check_snapshot(
            snap1, repo.locations(os.path.basename(step_and_repo)),
        )
        return snap1

    def test_download_evolving_multi_repos(self):
        with tempfile.TemporaryDirectory() as storage_dir:
            db_ctx = self._make_db_context()
            snap_dog, snap_cat, snap_dog2, snap_cat2 = (
                # Downloading a repo twice in a row should always be a
                # no-op, so we do that for all repos here just in case.
                self._download_repo_twice(
                    storage_dir, repo, step_and_repo, db_ctx,
                ) for repo, step_and_repo in [
                    (_GOOD_DOG, '0/good_dog'),
                    (_CHAOS_CAT, '0/chaos_cat'),
                    (_GOOD_DOG2, '1/good_dog'),
                    (_CHAOS_CAT2, '1/chaos_cat'),
                ]
            )

            # dog & dog2 agree except for carrot + sausage.  They are the
            # same repo at different points in time, so even the `location`
            # fields of `Rpm`s agree.
            for sid, rpm in snap_dog.storage_id_to_rpm.items():
                if '-carrot-' not in rpm.location:
                    self.assertEqual(rpm, snap_dog2.storage_id_to_rpm[sid])
            for sid, rpm in snap_dog2.storage_id_to_rpm.items():
                if '-sausage-' not in rpm.location:
                    self.assertEqual(rpm, snap_dog.storage_id_to_rpm[sid])
            self.assertEqual(
                len(snap_dog.storage_id_to_rpm),
                len(snap_dog2.storage_id_to_rpm),
            )

            # cat consists of sausage 3b, which also occurs in dog2
            (sid_3b, rpm_3b), = snap_cat.storage_id_to_rpm.items()
            self.assertEqual(
                _location_basename(rpm_3b),
                _location_basename(snap_dog2.storage_id_to_rpm[sid_3b]),
            )

            # The remaining 4 of 6 pairs have no overlap in storage IDs or RPMs
            for a_snap, b_snap in [
                (snap_dog, snap_cat),
                (snap_dog, snap_cat2),
                (snap_dog2, snap_cat2),
                (snap_cat, snap_cat2),
            ]:
                a = a_snap.storage_id_to_rpm
                b = b_snap.storage_id_to_rpm
                self.assertEqual(set(), set(a.keys()) & set(b.keys()))
                self.assertEqual(
                    set(),
                    {_location_basename(r) for r in a.values()} &
                    {_location_basename(r) for r in b.values()},
                )

    def _join_snapshots(self, snapshots: List[RepoSnapshot]) -> RepoSnapshot:
        # repomd & repodata should be the same across all shards
        repomd = snapshots[0].repomd
        storage_id_to_repodata = snapshots[0].storage_id_to_repodata
        storage_id_to_rpm = {}

        for snapshot in snapshots:
            self._check_repomd_equal(repomd, snapshot.repomd)
            self.assertEqual(
                storage_id_to_repodata, snapshot.storage_id_to_repodata,
            )
            for sid, rpm in snapshot.storage_id_to_rpm.items():
                set_new_key(storage_id_to_rpm, sid, rpm)

        return RepoSnapshot(
            repomd=repomd,
            storage_id_to_repodata=storage_id_to_repodata,
            storage_id_to_rpm=storage_id_to_rpm,
        )

    def _check_visitors_match_snapshot(
        self, visitors: List, snapshot: RepoSnapshot,
    ):
        self._check_snapshot(snapshot, _GOOD_DOG_LOCATIONS)

        # All visitors should have 1 repodata set
        visitor_repodatas, = {frozenset(v.repodatas) for v in visitors}

        # All visitors inspect all RPMs, downloaded or not.  But,
        # non-downloaded ones lack a canonical checksum.
        visitor_rpm_sets = {
            frozenset(r._replace(canonical_checksum=None) for r in v.rpms)
                for v in visitors
        }
        self.assertEqual(1, len(visitor_rpm_sets), visitor_rpm_sets)
        visitor_rpms = [
            r for v in visitors for r in v.rpms if r.canonical_checksum
        ]
        self.assertEqual(
            len(visitor_rpms), len(set(visitor_rpms)), visitor_rpms,
        )

        # Snapshot & visitors' repodatas agree
        self.assertEqual(
            set(snapshot.storage_id_to_repodata.values()), visitor_repodatas,
        )

        # Rpms agree between snapshot & visitor
        self.assertEqual(
            set(snapshot.storage_id_to_rpm.values()), set(visitor_rpms),
        )

        # If other fields get added, this reminds us to update the above test
        self._check_no_other_fields(
            snapshot, ['repomd', 'storage_id_to_repodata', 'storage_id_to_rpm'],
        )

    def test_visitor_matches_snapshot(self):

        class Visitor:

            def __init__(self):
                self.repomd = None
                self.rpms = set()
                self.repodatas = set()

            def visit_repomd(self, repomd):
                assert self.repomd is None
                self.repomd = repomd

            def visit_repodata(self, repodata):
                assert repodata not in self.repodatas
                self.repodatas.add(repodata)

            def visit_rpm(self, rpm):
                assert rpm not in self.rpms
                self.rpms.add(rpm)

        with tempfile.TemporaryDirectory() as storage_dir:
            downloader = self._make_downloader(storage_dir, '0/good_dog')
            visitor_all = Visitor()
            self._check_visitors_match_snapshot(
                [visitor_all], downloader.download(visitors=[visitor_all]),
            )

            visitors = [Visitor() for _ in range(10)]
            snapshots = [
                downloader.download(
                    visitors=[visitor], rpm_shard=RpmShard(shard, len(visitors))
                ) for shard, visitor in enumerate(visitors)
            ]
            # It's about 1:1000 that all 3 RPMs end up in one shard, and
            # since this test is deterministic, it won't be flaky.
            self.assertGreater(sum(
                bool(s.storage_id_to_rpm) for s in snapshots
            ), 1)
            self._check_visitors_match_snapshot(
                visitors, self._join_snapshots(snapshots),
            )

    def test_lose_repodata_commit_race(self):
        'We downloaded & stored a repodata, but in the meantime some other '
        'writer committed the same repodata.'

        original_maybe_store = RepoDBContext.maybe_store
        faked_objs = []

        def my_maybe_store(self, table, obj, storage_id):
            if re.match(FILELISTS_REPODATA_REGEX, obj.location):
                faked_objs.append(obj)
                return f'fake_already_stored_{obj.location}'
            return original_maybe_store(self, table, obj, storage_id)

        with mock.patch.object(
            RepoDBContext, 'maybe_store', new=my_maybe_store
        ), tempfile.TemporaryDirectory() as storage_dir:
            snapshot = self._make_downloader(
                storage_dir, '0/good_dog',
            ).download()
            faked_obj, = faked_objs
            # We should be using the winning writer's storage ID.
            self.assertEqual(
                faked_obj,
                snapshot.storage_id_to_repodata[
                    f'fake_already_stored_{faked_obj.location}'
                ],
            )

    def test_lose_rpm_commit_race(self):
        'We downloaded & stored an RPM, but in the meantime some other '
        'writer committed the same RPM.'
        original_get_canonical = RepoDBContext.get_rpm_canonical_checksums

        # When we download the RPM, the mock `my_maybe_store` writes a
        # single `Checksum` here, the canonical one for the `mice` RPM.
        # Then, during mutable RPM detection, the mock `my_get_canonical`
        # grabs this checksum (since it's not actually in the DB).
        mice_canonical_checksums = []
        # The mice object which was "previously stored" via the mocks
        mice_rpms = []

        def my_get_canonical(self, table, filename):
            if filename == 'rpm-test-mice-0.1-a.x86_64.rpm':
                return {mice_canonical_checksums[0]}
            return original_get_canonical(self, table, filename)

        original_maybe_store = RepoDBContext.maybe_store

        def my_maybe_store(self, table, obj, storage_id):
            if re.match(MICE_01_RPM_REGEX, obj.location):
                mice_rpms.append(obj)
                assert not mice_canonical_checksums, mice_canonical_checksums
                mice_canonical_checksums.append(obj.canonical_checksum)
                return f'fake_already_stored_{obj.location}'
            return original_maybe_store(self, table, obj, storage_id)

        with mock.patch.object(
            RepoDBContext, 'get_rpm_canonical_checksums', new=my_get_canonical,
        ), mock.patch.object(
            RepoDBContext, 'maybe_store', new=my_maybe_store
        ), tempfile.TemporaryDirectory() as storage_dir:
            snapshot = self._make_downloader(
                storage_dir, '0/good_dog'
            ).download()
            mice_rpm, = mice_rpms
            # We should be using the winning writer's storage ID.
            self.assertEqual(
                mice_rpm,
                snapshot.storage_id_to_rpm[
                    f'fake_already_stored_{mice_rpm.location}'
                ],
            )

    def test_mutable_rpm(self):
        with tempfile.TemporaryDirectory() as storage_dir:
            db_ctx = self._make_db_context()
            good_snapshot = self._make_downloader(
                storage_dir, '0/good_dog', db_ctx
            ).download()
            self._check_snapshot(good_snapshot, _GOOD_DOG_LOCATIONS)
            bad_snapshot = self._make_downloader(
                storage_dir, '0/bad_dog', db_ctx
            ).download()
            bad_location, = _BAD_DOG.locations('bad_dog')
            self._check_snapshot(
                bad_snapshot, [bad_location], has_errors=True,
            )

            # Check that the MutableRpmError is populated correctly.
            (error, bad_rpm), = bad_snapshot.storage_id_to_rpm.items()
            self.assertIsInstance(error, MutableRpmError)
            error_dict = dict(error.args)

            self.assertEqual(bad_location, bad_rpm.location)
            self.assertEqual(bad_location, error_dict['location'])

            self.assertRegex(error_dict['storage_id'], '^test:')
            self.assertEqual(
                str(bad_rpm.canonical_checksum), error_dict['checksum'],
            )
            good_rpm, = [
                r for r in good_snapshot.storage_id_to_rpm.values()
                    if '-milk-' in r.location
            ]
            self.assertEqual(
                [str(good_rpm.canonical_checksum)],
                error_dict['other_checksums'],
            )

            # Now, even fetching `good_dog` will show a mutable_rpm error.
            self._check_snapshot(
                self._make_downloader(
                    storage_dir, '0/good_dog', db_ctx
                ).download(), _GOOD_DOG_LOCATIONS, has_errors=True,
            )

            # But using the "deleted_mutable_rpms" facility, we can forget
            # about the error.
            with mock.patch.object(
                repo_downloader, 'deleted_mutable_rpms', new={
                    os.path.basename(bad_rpm.location): {
                        bad_rpm.canonical_checksum
                    },
                },
            ):
                self._check_snapshot(
                    self._make_downloader(
                        storage_dir, '0/good_dog', db_ctx
                    ).download(), _GOOD_DOG_LOCATIONS, has_errors=True,
                )
