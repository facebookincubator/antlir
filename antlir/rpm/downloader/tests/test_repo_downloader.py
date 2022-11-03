#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Future work:

  - We should assert how the DB changes as a consequence of our writes.
    Right now, the coverage for this is a bit slim.  First, we cover the
    basics in `test_repo_db.py`.  Second, some of these multi-download tests
    below would fail if we did something totally broken to the DB.
"""
import os
import re
import tempfile
import unittest
from contextlib import contextmanager
from functools import partial
from io import BytesIO
from typing import List, Tuple
from unittest import mock

import requests
import urllib3
from antlir.common import set_new_key
from antlir.fs_utils import temp_dir
from antlir.rpm.common import RpmShard
from antlir.rpm.db_connection import DBConnectionContext
from antlir.rpm.downloader import (
    common as downloader_common,
    repo_downloader,
    repodata_downloader,
    rpm_downloader,
)
from antlir.rpm.downloader.repomd_downloader import REPOMD_MAX_RETRY_S
from antlir.rpm.repo_db import RepodataTable, RepoDBContext
from antlir.rpm.repo_snapshot import (
    FileIntegrityError,
    HTTPError,
    MutableRpmError,
    RepoSnapshot,
)
from antlir.rpm.tests import temp_repos
from antlir.rpm.yum_dnf_conf import YumDnfConfRepo


def raise_fake_http_error(_contents):
    class FakeResponse:
        pass

    response = FakeResponse()
    response.status_code = 404
    raise requests.exceptions.HTTPError(response=response)


def _location_basename(rpm):
    return rpm._replace(location=os.path.basename(rpm.location))


# We default to spreading the downloads across 4 threads in tests
_THREADS = 4
MICE_01_RPM_REGEX = r".*/rpm-test-mice-0\.1-a\.x86_64\.rpm$"
# Used to target a specific RPM
_MICE_LOCATION = "good_dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm"
FILELISTS_REPODATA_REGEX = r"repodata/[0-9a-f]*-filelists\.xml\.gz$"

_GOOD_DOG = temp_repos.Repo(
    [
        # Copy-pasta'd from `temp_repos.py` to avoid unnecessary
        # cross-dependencies.
        temp_repos.Rpm("milk", "1.41", "42"),
        temp_repos.Rpm("mice", "0.1", "a"),
        temp_repos.Rpm("carrot", "2", "rc0"),
    ]
)
_GOOD_DOG_LOCATIONS = _GOOD_DOG.locations("good_dog")

# Make another happy-path repo to further test multiple repos
_FRIENDLY_FERRET = temp_repos.Repo(
    [temp_repos.Rpm("gouda", "1.89", "f"), temp_repos.Rpm("feta", "0.4", "99")]
)
# These next 2 exist to test updates to repos, and using multiple repos.
_SAUSAGE_3_BETA = temp_repos.Rpm("sausage", "3", "beta")
_GOOD_DOG2 = _GOOD_DOG._replace(
    # Remove "carrot", add "sausage".
    rpms=_GOOD_DOG.rpms[:-1]
    + [_SAUSAGE_3_BETA]
)
_CHAOS_CAT = temp_repos.Repo([_SAUSAGE_3_BETA])  # Test cross-repo duplicates.
# Older "sausage" than GOOD_DOG2, treated just as any other non-duplicate RPM.
_CHAOS_CAT2 = temp_repos.Repo([temp_repos.Rpm("sausage", "3", "alpha")])

# Tests "mutable RPM" errors
_BAD_DOG = temp_repos.Repo(
    [temp_repos.Rpm("milk", "1.41", "42", override_contents="differs from good_dog")]
)

# Tests case of having a repo with no RPMs
_EMPTY_EEL = temp_repos.Repo([])


class DownloadReposTestCase(unittest.TestCase):
    def log_sample(self, *args, **kwargs):
        pass

    @classmethod
    def setUpClass(cls):
        cls.multi_repo_dict = {
            "good_dog": _GOOD_DOG,
            "good_dog2": _GOOD_DOG2,
            "chaos_cat": _CHAOS_CAT,
            "chaos_cat2": _CHAOS_CAT2,
            "friendly_ferret": _FRIENDLY_FERRET,
        }

        # Since we only read the repo, it is much faster to create it once
        # for all the tests (~4x speed-up as of writing).
        cls.temp_repos_ctx = temp_repos.temp_repos_steps(
            gpg_signing_key=temp_repos.get_test_signing_key(),
            repo_change_steps=[
                {
                    "good_dog": _GOOD_DOG,
                    "chaos_cat": _CHAOS_CAT,
                    "bad_dog": _BAD_DOG,
                },
                {
                    "good_dog": _GOOD_DOG2,
                    "chaos_cat": _CHAOS_CAT2,
                    "empty_eel": _EMPTY_EEL,
                },
                cls.multi_repo_dict,
            ],
        )
        cls.repos_root = cls.temp_repos_ctx.__enter__()

    @classmethod
    def tearDownClass(cls):
        cls.temp_repos_ctx.__exit__(None, None, None)

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def _make_downloader_from_ctx(
        self, step_and_repo, tmp_db, dir_name, rpm_shard=None
    ):
        repo = YumDnfConfRepo(
            name=step_and_repo,
            base_url=(self.repos_root / step_and_repo).file_url(),
            gpg_key_urls=("not_used",),
        )
        return partial(
            repo_downloader.download_repos,
            repos_and_universes=[(repo, "fakeverse")],
            cfg=repo_downloader.DownloadConfig(
                db_cfg={"kind": "sqlite", "db_path": tmp_db.name},
                storage_cfg={
                    "key": "test",
                    "kind": "filesystem",
                    "base_dir": dir_name,
                },
                rpm_shard=rpm_shard or RpmShard(shard=0, modulo=1),
                threads=_THREADS,
            ),
            log_sample=self.log_sample,
        )

    @contextmanager
    def _make_downloader(self, step_and_repo):
        with tempfile.NamedTemporaryFile() as tmp_db, temp_dir() as storage_dir:
            yield self._make_downloader_from_ctx(step_and_repo, tmp_db, storage_dir)

    def _check_storage_id_error(self, storage_id_to_obj, error_cls):
        'Ensure exactly one of the objects has an "error" storage ID.'
        error_dict = None
        for sid, obj in storage_id_to_obj.items():
            if isinstance(sid, str):
                continue
            self.assertIsInstance(sid, error_cls)
            self.assertIsNone(error_dict)
            error_dict = dict(sid.args)
            self.assertEqual(obj.location, error_dict["location"])
        self.assertIsNotNone(error_dict)
        return error_dict

    def _check_no_other_fields(self, snapshot, fields):
        checked_fields = {field: None for field in fields}
        self.assertEqual(
            RepoSnapshot(**checked_fields), snapshot._replace(**checked_fields)
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
            snapshot, ["repomd", "storage_id_to_repodata", "storage_id_to_rpm"]
        )

        if not has_errors:
            for sito in [
                snapshot.storage_id_to_rpm,
                snapshot.storage_id_to_repodata,
            ]:
                self.assertTrue(all(isinstance(s, str) for s in sito))

    def _check_repomd_equal(self, a, b):
        self.assertEqual(
            a._replace(fetch_timestamp=None), b._replace(fetch_timestamp=None)
        )

    @contextmanager
    def _break_open_url(self, url_regex, corrupt_file_fn):
        original_open_url = downloader_common.open_url

        def my_open_url(url):
            if re.match(url_regex, url):
                with original_open_url(url) as f:
                    return BytesIO(corrupt_file_fn(f.read()))
            return original_open_url(url)

        with mock.patch.object(downloader_common, "open_url") as mock_fn:
            mock_fn.side_effect = my_open_url
            yield mock_fn

    @contextmanager
    def _check_download_error(self, url_regex, corrupt_file_fn, error_cls):
        with self._make_downloader("0/good_dog") as downloader:
            with self._break_open_url(url_regex, corrupt_file_fn):
                (res,) = list(downloader())
                bad_repo, bad_snapshot = res
                self.assertEqual("0/good_dog", bad_repo.name)
                self._check_snapshot(bad_snapshot, _GOOD_DOG_LOCATIONS, has_errors=True)
                (
                    storage_id_to_obj,
                ) = [  # Exactly one of RPMs & repodatas will have an error.
                    sito
                    for sito in [
                        bad_snapshot.storage_id_to_rpm,
                        bad_snapshot.storage_id_to_repodata,
                    ]
                    if any(not isinstance(sid, str) for sid in sito)
                ]
                yield self._check_storage_id_error(storage_id_to_obj, error_cls)
            (
                # Re-downloading outside of the mock results in the same
                # snapshot, but with the error corrected.
                res,
            ) = list(downloader())
            good_repo, good_snapshot = res
            self.assertEqual("0/good_dog", good_repo.name)
            self._check_snapshot(good_snapshot, _GOOD_DOG_LOCATIONS)
            self._check_repomd_equal(good_snapshot.repomd, bad_snapshot.repomd)
            for good_sito, bad_sito in [
                (
                    good_snapshot.storage_id_to_rpm,
                    bad_snapshot.storage_id_to_rpm,
                )
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

    @mock.patch("antlir.common._mockable_retry_fn_sleep", mock.Mock())
    def test_rpm_download_errors(self):

        extra_bytes = b"change size"
        with self._check_download_error(
            MICE_01_RPM_REGEX, lambda s: s + extra_bytes, FileIntegrityError
        ) as error_dict:
            self.assertEqual(_MICE_LOCATION, error_dict["location"])
            self.assertEqual("size", error_dict["failed_check"])
            self.assertEqual(
                len(extra_bytes),
                int(error_dict["actual"]) - int(error_dict["expected"]),
            )

        with self._check_download_error(
            MICE_01_RPM_REGEX,
            lambda s: b"dog" + s[3:],  # change contents
            FileIntegrityError,
        ) as error_dict:
            self.assertEqual(_MICE_LOCATION, error_dict["location"])
            self.assertEqual("sha256", error_dict["failed_check"])

    @mock.patch("antlir.common._mockable_retry_fn_sleep")
    def test_rpm_download_errors_no_retry(self, mock_sleep):
        with self._check_download_error(
            MICE_01_RPM_REGEX, raise_fake_http_error, HTTPError
        ) as error_dict:
            self.assertEqual(_MICE_LOCATION, error_dict["location"])
            self.assertEqual(404, error_dict["http_status"])
        # Shouldn't have retried for a 404 error
        mock_sleep.assert_not_called()

    @mock.patch("antlir.common._mockable_retry_fn_sleep")
    def test_repomd_download_error(self, mock_sleep):
        with self._make_downloader("0/good_dog") as downloader:
            with self._break_open_url(
                r".*repomd.xml", raise_fake_http_error
            ), self.assertRaises(HTTPError):
                next(downloader())
        self.assertEqual(len(REPOMD_MAX_RETRY_S), len(mock_sleep.call_args_list))

    @mock.patch("antlir.common._mockable_retry_fn_sleep")
    def test_rpm_download_errors_http_5xx(self, mock_sleep):
        def raise_5xx(_):
            raise requests.exceptions.HTTPError(response=mock.Mock(status_code=500))

        mice_location = "good_dog-pkgs/rpm-test-mice-0.1-a.x86_64.rpm"
        # Unlike 4xx errors, we retry 5xx ones (and 408s) on the presumption
        # that server errors may be transient and succeed in a future attempt
        with self._check_download_error(
            MICE_01_RPM_REGEX, raise_5xx, HTTPError
        ) as error_dict:
            self.assertEqual(mice_location, error_dict["location"])
            self.assertEqual(500, error_dict["http_status"])
            self.assertEqual(
                len(rpm_downloader.RPM_MAX_RETRY_S),
                len(mock_sleep.call_args_list),
            )

    @mock.patch("antlir.common._mockable_retry_fn_sleep")
    def test_rpm_download_errors_protocol_error(self, mock_sleep):
        def raise_protocol_error(_):
            raise urllib3.exceptions.ProtocolError(
                "blah blah",
                ConnectionResetError(104, "Connection reset by peer OF DOOM"),
            )

        with self._make_downloader("0/good_dog") as downloader:
            with self._break_open_url(
                MICE_01_RPM_REGEX, raise_protocol_error
            ), self.assertRaisesRegex(
                # We'll retry up to the max (assertion below), and then fail.
                urllib3.exceptions.ProtocolError,
                " OF DOOM",
            ):
                next(downloader())

        self.assertEqual(
            len(rpm_downloader.RPM_MAX_RETRY_S),
            len(mock_sleep.call_args_list),
        )

    @mock.patch("antlir.common._mockable_retry_fn_sleep", mock.Mock())
    def test_repodata_download_errors(self):
        with self._make_downloader("0/good_dog") as downloader:
            # This will raise a RepodataParseError, which is not a
            # ReportableError -- but this should still fail the snapshot.
            with self._break_open_url(
                r".*/good_dog/repodata/[0-9a-f]*-primary\.sqlite\.bz2$",
                lambda s: s[3:],  # change contents
            ), self.assertRaises(repodata_downloader.RepodataParseError):
                downloader()

        # Since RepodataParseError is not a ReportableError (but HTTPError is),
        # this is a different code path
        with self._make_downloader("0/good_dog") as downloader:
            with self._break_open_url(
                r".*/good_dog/repodata/[0-9a-f]*-primary\.sqlite\.bz2$",
                raise_fake_http_error,
            ), self.assertRaises(HTTPError):
                downloader()

        # Failure to get non-primary repodata should also abort a snapshot.
        with self._make_downloader("0/good_dog") as downloader:
            with self._break_open_url(
                r".*/good_dog/" + FILELISTS_REPODATA_REGEX,
                raise_fake_http_error,
            ), self.assertRaises(HTTPError):
                downloader()

    def _download_repo_twice(self, repo, step_and_repo, tmp_db, storage_dir):
        downloader = self._make_downloader_from_ctx(step_and_repo, tmp_db, storage_dir)
        (res1,) = list(downloader())
        repo1, snap1 = res1
        (res2,) = list(downloader())
        repo2, snap2 = res2
        self.assertEqual(repo1, repo2)
        self._check_repomd_equal(snap1.repomd, snap2.repomd)
        self.assertEqual(snap1._replace(repomd=None), snap2._replace(repomd=None))
        self._check_snapshot(snap1, repo.locations(os.path.basename(step_and_repo)))
        return snap1

    def test_download_evolving_multi_repos(self):
        with tempfile.NamedTemporaryFile() as tmp_db, temp_dir() as storage_dir:
            snap_dog, snap_cat, snap_dog2, snap_cat2 = (
                # Downloading a repo twice in a row should always be a
                # no-op, so we do that for all repos here just in case.
                self._download_repo_twice(repo, step_and_repo, tmp_db, storage_dir)
                for repo, step_and_repo in [
                    (_GOOD_DOG, "0/good_dog"),
                    (_CHAOS_CAT, "0/chaos_cat"),
                    (_GOOD_DOG2, "1/good_dog"),
                    (_CHAOS_CAT2, "1/chaos_cat"),
                ]
            )

        # dog & dog2 agree except for carrot + sausage.  They are the
        # same repo at different points in time, so even the `location`
        # fields of `Rpm`s agree.
        for sid, rpm in snap_dog.storage_id_to_rpm.items():
            if "-carrot-" not in rpm.location:
                self.assertEqual(rpm, snap_dog2.storage_id_to_rpm[sid])
        for sid, rpm in snap_dog2.storage_id_to_rpm.items():
            if "-sausage-" not in rpm.location:
                self.assertEqual(rpm, snap_dog.storage_id_to_rpm[sid])
        self.assertEqual(
            len(snap_dog.storage_id_to_rpm), len(snap_dog2.storage_id_to_rpm)
        )
        (
            # cat consists of sausage 3b, which also occurs in dog2
            (sid_3b, rpm_3b),
        ) = snap_cat.storage_id_to_rpm.items()
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
                {_location_basename(r) for r in a.values()}
                & {_location_basename(r) for r in b.values()},
            )

    def _reduce_equal_snapshots(
        self, repo_snapshots: List[Tuple[YumDnfConfRepo, RepoSnapshot]]
    ) -> RepoSnapshot:
        self.assertGreater(len(repo_snapshots), 0)
        # repo, repomd & repodata should be the same across all shards
        head_repo, head_snapshot = repo_snapshots[0]
        head_repomd = head_snapshot.repomd
        head_storage_id_to_repodata = head_snapshot.storage_id_to_repodata
        storage_id_to_rpm = {}

        for repo, snapshot in repo_snapshots[1:]:
            self.assertEqual(head_repo, repo)
            self._check_repomd_equal(head_repomd, snapshot.repomd)
            self.assertEqual(
                head_storage_id_to_repodata, snapshot.storage_id_to_repodata
            )
            for sid, rpm in snapshot.storage_id_to_rpm.items():
                set_new_key(storage_id_to_rpm, sid, rpm)

        return RepoSnapshot(
            repomd=head_repomd,
            storage_id_to_repodata=head_storage_id_to_repodata,
            storage_id_to_rpm=storage_id_to_rpm,
        )

    def _check_visitors_match_snapshot(self, visitors: List, snapshot: RepoSnapshot):
        self._check_snapshot(snapshot, _GOOD_DOG_LOCATIONS)
        (
            # All visitors should have 1 repodata set
            visitor_repodatas,
        ) = {frozenset(v.repodatas) for v in visitors}

        # All visitors inspect all RPMs, downloaded or not.  But,
        # non-downloaded ones lack a canonical checksum.
        visitor_rpm_sets = {
            frozenset(r._replace(canonical_checksum=None) for r in v.rpms)
            for v in visitors
        }
        self.assertEqual(1, len(visitor_rpm_sets), visitor_rpm_sets)
        visitor_rpms = [r for v in visitors for r in v.rpms if r.canonical_checksum]
        self.assertEqual(len(visitor_rpms), len(set(visitor_rpms)), visitor_rpms)

        # Snapshot & visitors' repodatas agree
        self.assertEqual(
            set(snapshot.storage_id_to_repodata.values()), visitor_repodatas
        )

        # Rpms agree between snapshot & visitor
        self.assertEqual(set(snapshot.storage_id_to_rpm.values()), set(visitor_rpms))

        # If other fields get added, this reminds us to update the above test
        self._check_no_other_fields(
            snapshot, ["repomd", "storage_id_to_repodata", "storage_id_to_rpm"]
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

        with tempfile.NamedTemporaryFile() as tmp_db, temp_dir() as storage_dir:
            partial_downloader = partial(
                self._make_downloader_from_ctx,
                "0/good_dog",
                tmp_db,
                storage_dir,
            )
            visitor_all = Visitor()
            (res,) = list(partial_downloader()(visitors=[visitor_all]))
            repo, snapshot = res
            self._check_visitors_match_snapshot([visitor_all], snapshot)

            visitors = [Visitor() for _ in range(10)]
            repo_snapshots = []
            for shard, visitor in enumerate(visitors):
                repo_snapshots += partial_downloader(RpmShard(shard, len(visitors)))(
                    visitors=[visitor]
                )

            # It's about 1:1000 that all 3 RPMs end up in one shard, and
            # since this test is deterministic, it won't be flaky.
            self.assertGreater(
                sum(bool(s.storage_id_to_rpm) for _, s in repo_snapshots), 1
            )
            self._check_visitors_match_snapshot(
                visitors, self._reduce_equal_snapshots(repo_snapshots)
            )

    def test_lose_repodata_commit_race(self):
        "We downloaded & stored a repodata, but in the meantime some other"
        "writer committed the same repodata."

        original_maybe_store = RepoDBContext.maybe_store
        faked_objs = []

        def my_maybe_store(self, table, obj, storage_id):
            if re.match(FILELISTS_REPODATA_REGEX, obj.location):
                faked_objs.append(obj)
                return f"fake_already_stored_{obj.location}"
            return original_maybe_store(self, table, obj, storage_id)

        with mock.patch.object(
            RepoDBContext, "maybe_store", new=my_maybe_store
        ), self._make_downloader("0/good_dog") as downloader:
            (res,) = list(downloader())
            _, snapshot = res
            (faked_obj,) = faked_objs
            # We should be using the winning writer's storage ID.
            self.assertEqual(
                faked_obj,
                snapshot.storage_id_to_repodata[
                    f"fake_already_stored_{faked_obj.location}"
                ],
            )

    def test_lose_rpm_commit_race(self):
        "We downloaded & stored an RPM, but in the meantime some other"
        "writer committed the same RPM."
        original_get_canonical = RepoDBContext.get_rpm_canonical_checksums_per_universe

        # When we download the RPM, the mock `my_maybe_store` writes a
        # single `Checksum` here, the canonical one for the `mice` RPM.
        # Then, during mutable RPM detection, the mock `my_get_canonical`
        # grabs this checksum (since it's not actually in the DB).
        mice_canonical_checksums = []
        # The mice object which was "previously stored" via the mocks
        mice_rpms = []

        def my_get_canonical(self, table, rpm, all_snapshot_universes):
            if rpm.nevra() == "rpm-test-mice-0:0.1-a.x86_64":
                return {(mice_canonical_checksums[0], "fakeverse")}
            return original_get_canonical(self, table, rpm, all_snapshot_universes)

        original_maybe_store = RepoDBContext.maybe_store

        def my_maybe_store(self, table, obj, storage_id):
            if re.match(MICE_01_RPM_REGEX, obj.location):
                mice_rpms.append(obj)
                assert not mice_canonical_checksums, mice_canonical_checksums
                mice_canonical_checksums.append(obj.canonical_checksum)
                return f"fake_already_stored_{obj.location}"
            return original_maybe_store(self, table, obj, storage_id)

        with mock.patch.object(
            RepoDBContext,
            "get_rpm_canonical_checksums_per_universe",
            new=my_get_canonical,
        ), mock.patch.object(
            RepoDBContext, "maybe_store", new=my_maybe_store
        ), self._make_downloader(
            "0/good_dog"
        ) as downloader:
            (res,) = list(downloader())
            _, snapshot = res
            (mice_rpm,) = mice_rpms
            # We should be using the winning writer's storage ID.
            self.assertEqual(
                mice_rpm,
                snapshot.storage_id_to_rpm[f"fake_already_stored_{mice_rpm.location}"],
            )

    # Test case of having dangling repodata refs without a repomd
    def test_dangling_repodatas(self):
        orig_rd = repodata_downloader._download_repodata
        with mock.patch.object(
            RepoDBContext, "store_repomd"
        ) as mock_store, mock.patch.object(
            repodata_downloader, "_download_repodata"
        ) as mock_rd, tempfile.NamedTemporaryFile() as tmp_db, temp_dir() as storage_dir:  # noqa: E501
            db_cfg = {
                "kind": "sqlite",
                "db_path": tmp_db.name,
                "readonly": False,
            }
            mock_rd.side_effect = orig_rd
            mock_store.side_effect = RuntimeError
            with self.assertRaises(RuntimeError):
                next(
                    self._make_downloader_from_ctx("0/good_dog", tmp_db, storage_dir)()
                )
            # Get the repodatas that the mocked fn was passed
            called_rds = [x[0][0] for x in mock_rd.call_args_list]
            db_conn = DBConnectionContext.from_json(db_cfg)
            db_ctx = RepoDBContext(db_conn, db_conn.SQL_DIALECT)
            repodata_table = RepodataTable()
            with db_ctx as repo_db_ctx:
                storage_ids = [
                    repo_db_ctx.get_storage_id(repodata_table, rd) for rd in called_rds
                ]
            # All of these repodatas got stored in the db
            self.assertEqual(len(called_rds), len(storage_ids))

            # Now ensure that no repomds got inserted (i.e. nothing to
            # reference the above repodatas)
            mock_store.assert_called_once()
            with db_ctx as repo_db_ctx:
                with repo_db_ctx._cursor() as cursor:
                    cursor.execute("SELECT COUNT(*) from repo_metadata")
                    res = cursor.fetchone()
            self.assertEqual(0, res[0])

    def test_mutable_rpm(self):
        with tempfile.NamedTemporaryFile() as tmp_db, temp_dir() as storage_dir:
            (good_res,) = list(
                self._make_downloader_from_ctx("0/good_dog", tmp_db, storage_dir)()
            )
            _, good_snapshot = good_res
            self._check_snapshot(good_snapshot, _GOOD_DOG_LOCATIONS)
            (bad_res,) = list(
                self._make_downloader_from_ctx("0/bad_dog", tmp_db, storage_dir)()
            )
            _, bad_snapshot = bad_res
            (bad_location,) = _BAD_DOG.locations("bad_dog")
            self._check_snapshot(bad_snapshot, [bad_location], has_errors=True)
            (
                # Check that the MutableRpmError is populated correctly.
                (error, bad_rpm),
            ) = bad_snapshot.storage_id_to_rpm.items()
            self.assertIsInstance(error, MutableRpmError)
            error_dict = dict(error.args)

            self.assertEqual(bad_location, bad_rpm.location)
            self.assertEqual(bad_location, error_dict["location"])

            self.assertRegex(error_dict["storage_id"], "^test:")
            self.assertEqual(str(bad_rpm.canonical_checksum), error_dict["checksum"])
            (good_rpm,) = [
                r
                for r in good_snapshot.storage_id_to_rpm.values()
                if "-milk-" in r.location
            ]
            self.assertEqual(
                [(str(good_rpm.canonical_checksum), "fakeverse")],
                error_dict["other_checksums_and_universes"],
            )

            # Now, even fetching `good_dog` will show a mutable_rpm error.
            self._check_snapshot(
                list(
                    self._make_downloader_from_ctx("0/good_dog", tmp_db, storage_dir)()
                )[0][1],
                _GOOD_DOG_LOCATIONS,
                has_errors=True,
            )

            # But using the "deleted_mutable_rpms" facility, we can forget
            # about the error.
            with mock.patch.object(
                rpm_downloader,
                "deleted_mutable_rpms",
                new={os.path.basename(bad_rpm.location): {bad_rpm.canonical_checksum}},
            ):
                self._check_snapshot(
                    list(
                        self._make_downloader_from_ctx(
                            "0/good_dog", tmp_db, storage_dir
                        )()
                    )[0][1],
                    _GOOD_DOG_LOCATIONS,
                    has_errors=True,
                )

    def test_download_multiple_repos(self):
        with tempfile.NamedTemporaryFile() as tmp_db, tempfile.TemporaryDirectory() as storage_dir:  # noqa: E501
            repos = [
                YumDnfConfRepo(
                    name=repo,
                    base_url=(self.repos_root / "2" / repo).file_url(),
                    gpg_key_urls=("not_used",),
                )
                for repo in self.multi_repo_dict.keys()
            ]
            repo_snapshots = list(
                repo_downloader.download_repos(
                    repos_and_universes=[(repo, "fakeverse") for repo in repos],
                    cfg=repo_downloader.DownloadConfig(
                        db_cfg={"kind": "sqlite", "db_path": tmp_db.name},
                        storage_cfg={
                            "key": "test",
                            "kind": "filesystem",
                            "base_dir": storage_dir,
                        },
                        rpm_shard=RpmShard(shard=0, modulo=1),
                        threads=_THREADS,
                    ),
                    log_sample=self.log_sample,
                )
            )
            # Each snapshot will have unique repomds
            self.assertEqual(
                len(repo_snapshots), len({x.repomd for _, x in repo_snapshots})
            )
            # Assert rpm locations in the snapshot match expectations
            self.assertEqual(
                {k: sorted(v.locations(k)) for k, v in self.multi_repo_dict.items()},
                {
                    r.name: sorted(rpm.location for rpm in s.storage_id_to_rpm.values())
                    for r, s in repo_snapshots
                },
            )
            # We have duplicate RPMs across snapshots..
            # First, get the list of all RPMs
            fake_rpms = [
                rpm for repo in self.multi_repo_dict.values() for rpm in repo.rpms
            ]
            # Next extract the number of unique RPMs
            unique_fake_rpms = len(set(fake_rpms))
            # Ensure we do in fact have duplicates
            self.assertLess(unique_fake_rpms, len(fake_rpms))
            self.assertGreater(unique_fake_rpms, 0)
            # Now get the amount of duplicates in the snapshot
            unique_storage_ids = {
                sid
                for _, snapshot in repo_snapshots
                for sid in snapshot.storage_id_to_rpm.keys()
            }
            # This is essentially showing that the storage_id was correctly
            # reused for duplicate RPMs
            self.assertEqual(unique_fake_rpms, len(unique_storage_ids))

    def test_download_changing_repomds(self):
        original_open_url = downloader_common.open_url
        i = 0

        def my_open_url(url):
            nonlocal i
            postfix = "repodata/repomd.xml"
            if postfix in url:
                i += 1
                return original_open_url(re.sub(r"/(\d)/", rf"/{i % 2}/", url))
            return original_open_url(url)

        with mock.patch.object(downloader_common, "open_url") as mock_fn:
            mock_fn.side_effect = my_open_url
            with self._make_downloader("0/good_dog") as downloader:
                with self.assertRaisesRegex(RuntimeError, "Integrity issue with repos"):
                    list(downloader())

    def test_empty_repo(self):
        with self._make_downloader("1/empty_eel") as downloader:
            res = list(downloader())
        self.assertEqual(1, len(res))
        repo, snapshot = res[0]
        self.assertEqual("1/empty_eel", repo.name)
        self.assertFalse(snapshot.storage_id_to_rpm)
        self._check_snapshot(snapshot, rpm_locations=[])
