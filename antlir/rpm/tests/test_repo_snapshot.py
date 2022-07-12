#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import sqlite3
import unittest
import unittest.mock

from antlir.fs_utils import temp_dir

from antlir.rpm.common import Checksum
from antlir.rpm.repo_objects import Repodata, RepoMetadata, Rpm
from antlir.rpm.repo_snapshot import (
    FileIntegrityError,
    HTTPError,
    MutableRpmError,
    RepoSnapshot,
)
from antlir.rpm.storage.filesystem_storage import FilesystemStorage


def _get_db_rows(db: sqlite3.Connection, table: str):
    cur = db.execute(f'SELECT * FROM "{table}"')
    return [
        {desc[0]: val for desc, val in zip(cur.description, row)}
        for row in cur.fetchall()
    ]


class RepoSnapshotTestCase(unittest.TestCase):
    def setUp(self) -> None:  # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def test_serialize_and_visit(self) -> None:
        repodata = Repodata(
            location="repodata_loc",
            checksum=Checksum("a", "b"),
            size=123,
            build_timestamp=456,
        )
        repomd = RepoMetadata(
            xml=b"foo",
            # This object is only as populated as this test requires, in
            # practice these would not be None.
            # pyre-fixme[6]: For 2nd param expected `int` but got `None`.
            fetch_timestamp=None,
            # pyre-fixme[6]: For 3rd param expected `List[Repodata]` but got `None`.
            repodatas=None,
            # pyre-fixme[6]: For 4th param expected `Checksum` but got `None`.
            checksum=None,
            # pyre-fixme[6]: For 5th param expected `int` but got `None`.
            size=None,
            build_timestamp=7654321,
        )
        rpm_base = Rpm(  # Reuse this template for all RPMs
            epoch=37,
            name="foo-bar",
            version="3.14",
            release="rc0",
            arch="i386",
            build_timestamp=90,
            canonical_checksum=Checksum("e", "f"),
            checksum=Checksum("c", "d"),
            # pyre-fixme[6]: For 9th param expected `str` but got `None`.
            location=None,  # `_replace`d below
            size=78,
            source_rpm="foo-bar-3.14-rc0.src.rpm",
        )
        rpm_normal = rpm_base._replace(location="normal.rpm")
        rpm_file_integrity = rpm_base._replace(location="file_integrity_error")
        rpm_http = rpm_base._replace(location="http_error")
        rpm_mutable = rpm_base._replace(location="mutable_rpm_error")
        error_file_integrity = FileIntegrityError(
            location=rpm_file_integrity.location,
            failed_check="size",
            expected=42,
            actual=7,
        )
        error_http = HTTPError(location=rpm_http.location, http_status=404)
        error_mutable_rpm = MutableRpmError(
            location=rpm_mutable.location,
            storage_id="rpm_mutable_sid",
            checksum=Checksum("g", "h"),
            other_checksums_and_universes={
                (Checksum("i", "j"), "u1"),
                (Checksum("k", "l"), "u2"),
            },
        )
        snapshot = RepoSnapshot(
            repomd=repomd,
            storage_id_to_repodata={"repodata_sid": repodata},
            storage_id_to_rpm={
                "rpm_normal_sid": rpm_normal,
                error_file_integrity: rpm_file_integrity,
                error_http: rpm_http,
                error_mutable_rpm: rpm_mutable,
            },
        )

        # Check the `to_sqlite` serialization
        with temp_dir() as td:
            storage = FilesystemStorage(key="test", base_dir=td / "storage")
            os.mkdir(td / "snapshot")
            # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
            with RepoSnapshot.add_sqlite_to_storage(
                storage, td / "snapshot"
            ) as db:
                snapshot.to_sqlite("fake_repo", db)
            with sqlite3.connect(
                RepoSnapshot.fetch_sqlite_from_storage(
                    storage, td / "snapshot", td / "snapshot.sql3"
                )
            ) as db:
                self.assertEqual(
                    [
                        {
                            "repo": "fake_repo",
                            "metadata_xml": repomd.xml.decode(),
                            "build_timestamp": repomd.build_timestamp,
                        }
                    ],
                    _get_db_rows(db, "repomd"),
                )

                self.assertEqual(
                    [
                        {
                            "repo": "fake_repo",
                            "path": "repodata_loc",
                            "build_timestamp": 456,
                            "checksum": "a:b",
                            "error": None,
                            "error_json": None,
                            "size": 123,
                            "storage_id": "repodata_sid",
                        }
                    ],
                    _get_db_rows(db, "repodata"),
                )

                base_row = {
                    "repo": "fake_repo",
                    "epoch": rpm_base.epoch,
                    "name": rpm_base.name,
                    "version": rpm_base.version,
                    "release": rpm_base.release,
                    "arch": rpm_base.arch,
                    "build_timestamp": rpm_base.build_timestamp,
                    "checksum": str(rpm_base.best_checksum()),
                    "size": rpm_base.size,
                    "source_rpm": rpm_base.source_rpm,
                }
                file_integrity_err_msg = error_file_integrity.to_dict()[
                    "message"
                ]
                self.assertEqual(
                    sorted(
                        json.dumps(row, sort_keys=True)
                        for row in [
                            {
                                **base_row,
                                "path": rpm_normal.location,
                                "error": None,
                                "error_json": None,
                                "storage_id": "rpm_normal_sid",
                            },
                            {
                                **base_row,
                                "path": rpm_file_integrity.location,
                                "error": "file_integrity",
                                "error_json": json.dumps(
                                    {
                                        "message": file_integrity_err_msg,
                                        "location": rpm_file_integrity.location,
                                        "failed_check": "size",
                                        # These are stringified because they
                                        # might have been checksums...  seems OK
                                        # for now.
                                        "expected": "42",
                                        "actual": "7",
                                    },
                                    sort_keys=True,
                                ),
                                "storage_id": None,
                            },
                            {
                                **base_row,
                                "path": rpm_http.location,
                                "error": "http",
                                "error_json": json.dumps(
                                    {
                                        "message": error_http.to_dict()[
                                            "message"
                                        ],
                                        "location": rpm_http.location,
                                        "http_status": 404,
                                    },
                                    sort_keys=True,
                                ),
                                "storage_id": None,
                            },
                            {
                                **base_row,
                                "path": rpm_mutable.location,
                                "error": "mutable_rpm",
                                "error_json": json.dumps(
                                    {
                                        "message": error_mutable_rpm.to_dict()[
                                            "message"
                                        ],
                                        "location": rpm_mutable.location,
                                        "checksum": "g:h",
                                        "other_checksums_and_universes": [
                                            ["i:j", "u1"],
                                            ["k:l", "u2"],
                                        ],
                                    },
                                    sort_keys=True,
                                ),
                                "storage_id": "rpm_mutable_sid",
                            },
                        ]
                    ),
                    sorted(
                        json.dumps(row, sort_keys=True)
                        for row in _get_db_rows(db, "rpm")
                    ),
                )

        # Check the visitor
        mock = unittest.mock.MagicMock()
        snapshot.visit(mock)
        mock.visit_repomd.assert_called_once_with(repomd)
        mock.visit_repodata.assert_called_once_with(repodata)
        rpm_calls = set()
        for name, args, kwargs in mock.visit_rpm.mock_calls:
            self.assertEqual("", name)
            self.assertEqual({}, kwargs)
            self.assertEqual(1, len(args))
            rpm_calls.add(args[0])
        self.assertEqual(
            rpm_calls, {rpm_normal, rpm_file_integrity, rpm_http, rpm_mutable}
        )
