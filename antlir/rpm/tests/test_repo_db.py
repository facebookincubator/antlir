#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import re
import tempfile
import unittest
from contextlib import contextmanager
from unittest import mock

from ..common import Checksum
from ..db_connection import DBConnectionContext
from ..repo_db import RepodataTable, RepoDBContext, RpmTable, SQLDialect
from ..repo_objects import Repodata, RepoMetadata, Rpm


_FAKE_RPM = Rpm(
    name="fake",
    epoch=0,
    version="a",
    release="3b",
    arch="aarch64",
    build_timestamp=37,
    # pyre-fixme[6]: Expected `Checksum` for 7th param but got `None`.
    checksum=None,  # populated separately by each test
    # pyre-fixme[6]: Expected `Checksum` for 8th param but got `None`.
    canonical_checksum=None,  # populated separately by each test
    # pyre-fixme[6]: Expected `str` for 9th param but got `None`.
    location=None,  # not used by repo_db code
    size=1337,
    # pyre-fixme[6]: Expected `str` for 11th param but got `None`.
    source_rpm=None,
)


def _get_schema(conn):
    return conn.execute(
        'SELECT `name`, `sql` FROM `sqlite_master` where `type` = "table"'
    ).fetchall()


class RepoDBTestCase(unittest.TestCase):
    def setUp(self) -> None:
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def _check_schema(self, conn) -> None:
        for (a_name, a_sql), (e_name, e_sql) in zip(
            _get_schema(conn),
            [
                (
                    "rpm",
                    (
                        "CREATE TABLE `rpm` ("
                        " `name` TEXT NOT NULL,"
                        " `epoch` INTEGER NOT NULL,"
                        " `version` TEXT NOT NULL,"
                        " `release` TEXT NOT NULL,"
                        " `arch` TEXT NOT NULL,"
                        " `universe` TEXT NOT NULL,"
                        " `checksum` TEXT NOT NULL,"
                        " `canonical_checksum` TEXT NOT NULL,"
                        " `size` INTEGER NOT NULL,"
                        " `build_timestamp` INTEGER NOT NULL,"
                        " `storage_id` TEXT NOT NULL,"
                        " PRIMARY KEY (`name`, `epoch`, `version`, `release`, "
                        "`arch`, `universe`, `checksum`)"
                        " )"
                    ),
                ),
                (
                    "repodata",
                    (
                        "CREATE TABLE `repodata` ("
                        " `checksum` TEXT NOT NULL,"
                        " `size` INTEGER NOT NULL,"
                        " `build_timestamp` INTEGER NOT NULL,"
                        " `storage_id` TEXT NOT NULL,"
                        " PRIMARY KEY (`checksum`)"
                        " )"
                    ),
                ),
                (
                    "repo_metadata",
                    (
                        "CREATE TABLE `repo_metadata` ("
                        " `universe` TEXT NOT NULL,"
                        " `repo` TEXT NOT NULL,"
                        " `fetch_timestamp` INTEGER NOT NULL,"
                        " `build_timestamp` INTEGER NOT NULL,"
                        " `checksum` TEXT NOT NULL,"
                        " `xml` BLOB NOT NULL,"
                        " PRIMARY KEY (`universe`, `repo`, `fetch_timestamp`, "
                        "`checksum`),"
                        " UNIQUE (`universe`, `repo`, `checksum`)"
                        " )"
                    ),
                ),
            ],
        ):
            self.assertEqual(e_name, a_name)
            self.assertEqual(e_sql, re.sub(r"\s+", " ", a_sql))

    @contextmanager
    def _make_conn_ctx(self):
        with tempfile.NamedTemporaryFile() as tf:
            yield DBConnectionContext.make(
                kind="sqlite", db_path=tf.name, readonly=False
            )

    @contextmanager
    def _make_db_ctx(self):
        with self._make_conn_ctx() as conn_ctx:
            with RepoDBContext(conn_ctx, SQLDialect.SQLITE3) as db_ctx:
                db_ctx.ensure_tables_exist()
                yield db_ctx

    def test_create_tables(self) -> None:
        with self._make_conn_ctx() as conn_ctx:
            # At first, there are no tables.
            with conn_ctx as conn:
                self.assertEqual([], _get_schema(conn))

            # The two iterations test different scenarios:
            # 0: The tables already existed, creating context again is a no-op.
            # 1: Creating the context will ensures that all tables exist.
            for _ in range(2):
                with RepoDBContext(conn_ctx, SQLDialect.SQLITE3) as db_ctx:
                    db_ctx.ensure_tables_exist()
                with conn_ctx as conn:
                    self._check_schema(conn)

    def _fake_repomd(self, fetch_timestamp):
        repomd_xml = b"""
        <repomd>
          <data type="primary_db">
            <checksum type="fakealgo">fakesum</checksum>
            <location href="repodata/fakesum-primary.sqlite.bz2"/>
            <timestamp>12345</timestamp>
            <size>555555</size>
          </data>
        </repomd>
        """
        with mock.patch("time.time") as mock_time:
            mock_time.return_value = fetch_timestamp
            repomd = RepoMetadata.new(xml=repomd_xml)
        return repomd

    def test_store_repomd_and_commit(self) -> None:
        repomd37 = self._fake_repomd(37)
        repomd73 = self._fake_repomd(73)
        self.assertGreater(repomd73.fetch_timestamp, repomd37.fetch_timestamp)

        with self._make_conn_ctx() as conn_ctx:
            # Exercise both the code path where our repomd to insert wins (gets
            # inserted), and the path where a racing writer had already inserted
            # the same repomd.
            for insert_repomd, db_repomd, do_commit in [
                (repomd37, repomd37, False),
                (repomd73, repomd73, False),
                # 37 is committed, won't be overwritten
                (repomd37, repomd37, True),
                (repomd73, repomd37, False),
                (repomd73, repomd37, True),
                (repomd37, repomd37, True),
            ]:
                with self.subTest(
                    insert_t=insert_repomd.fetch_timestamp,
                    db_t=db_repomd.fetch_timestamp,
                    do_commit=do_commit,
                ), RepoDBContext(conn_ctx, SQLDialect.SQLITE3) as db_ctx:
                    db_ctx.ensure_tables_exist()
                    self.assertEqual(
                        db_repomd.fetch_timestamp,
                        db_ctx.store_repomd(
                            "fakevers", "fake_repo", insert_repomd
                        ),
                    )
                    if do_commit:
                        db_ctx.commit()

    def _check_maybe_store_and_get_storage_id(self, table, obj) -> None:
        with self._make_db_ctx() as db_ctx:
            self.assertIs(None, db_ctx.get_storage_id(table, obj))
            self.assertEqual("fake1", db_ctx.maybe_store(table, obj, "fake1"))
            self.assertEqual("fake1", db_ctx.get_storage_id(table, obj))
            # This was already stored, so return the old storage ID.
            self.assertEqual("fake1", db_ctx.maybe_store(table, obj, "fake2"))
            # It is also possible to have an near-identical repodata index
            # with an earlier `build_timestamp`.
            if isinstance(obj, Repodata):
                self.assertEqual(
                    "fake1",
                    db_ctx.get_storage_id(
                        table,
                        obj._replace(build_timestamp=obj.build_timestamp + 1),
                    ),
                )

    def test_repodata_maybe_store_and_get_storage_id(self) -> None:
        self._check_maybe_store_and_get_storage_id(
            RepodataTable(),
            Repodata(
                location="repodata/fake.sqlite.gz",
                checksum=Checksum("fake", "fake"),
                size=1337,
                build_timestamp=37,
            ),
        )

    def test_rpm_maybe_store_and_get_storage_id(self) -> None:
        # NB: For RPMs, only `maybe_store` is used as part of the public API.
        self._check_maybe_store_and_get_storage_id(
            RpmTable("fake.verse"),
            _FAKE_RPM._replace(
                checksum=Checksum("fake", "fake"),
                canonical_checksum=Checksum("fake", "fake"),
            ),
        )

    def test_get_rpm_storage_id_and_checksum(self) -> None:
        table = RpmTable("fakeverse")
        # We'll have two entries for the same exact RPM, but the different
        # repos that contain it will have computed different checksums.
        rpm1 = _FAKE_RPM._replace(
            checksum=Checksum("fa", "ke1"),
            # At this point, we are trying to look this up:
            canonical_checksum=None,
        )
        rpm2 = rpm1._replace(checksum=Checksum("fa", "ke2"))
        canonical = Checksum("can", "onical")
        # It is also OK to have the checksum be the same as the canonical one.
        rpm_canon = rpm1._replace(checksum=canonical)
        with self._make_db_ctx() as db_ctx:
            # Nothing was inserted, yet.
            self.assertEqual(
                (None, None),
                db_ctx.get_rpm_storage_id_and_checksum(table, rpm1),
            )
            # We'll insert the RPM with its different checksums.
            insertion_order = [rpm_canon, rpm1, rpm2]
            for idx, inserted_rpm in enumerate(insertion_order):
                self.assertEqual(
                    "fake_sid",
                    db_ctx.maybe_store(
                        table,
                        inserted_rpm._replace(canonical_checksum=canonical),
                        "fake_sid",
                    ),
                )
                # Looking up by any inserted RPM checksum gets the same result.
                for rpm in insertion_order[: idx + 1]:
                    self.assertEqual(
                        ("fake_sid", canonical),
                        db_ctx.get_rpm_storage_id_and_checksum(table, rpm),
                    )

    def test_get_rpm_canonical_checksums(self) -> None:
        table = RpmTable("fakeverse")
        canonical1 = Checksum("can", "onical1")
        canonical2 = Checksum("can", "onical2")
        with self._make_db_ctx() as db_ctx:
            # These two entries into the `rpm` table refer to the same RPM
            # (same canonical checksum), but this illustrates that the
            # contents of such an RPM will currently be stored twice.
            self.assertEqual(
                "sid_same1",
                db_ctx.maybe_store(
                    table,
                    _FAKE_RPM._replace(
                        checksum=Checksum("fa", "ke1"),
                        canonical_checksum=canonical1,
                    ),
                    "sid_same1",
                ),
            )
            self.assertEqual(
                "sid_same2",
                db_ctx.maybe_store(
                    table,
                    _FAKE_RPM._replace(
                        checksum=Checksum("fa", "ke2"),
                        canonical_checksum=canonical1,
                    ),
                    "sid_same2",
                ),
            )

            # This here is an actual bug in the RPM repos: same RPM filename,
            # but different contents. Uh-oh.
            self.assertEqual(
                "sid_diff",
                db_ctx.maybe_store(
                    table,
                    _FAKE_RPM._replace(
                        checksum=Checksum("fa", "ke3"),
                        canonical_checksum=canonical2,
                    ),
                    "sid_diff",
                ),
            )

            # Whew, we can detect this mutable RPM file in our repos.
            self.assertEqual(
                {(canonical1, "fakeverse"), (canonical2, "fakeverse")},
                set(
                    db_ctx.get_rpm_canonical_checksums_per_universe(
                        table, _FAKE_RPM, {"fakeverse"}
                    )
                ),
            )

    def test_universe_charset(self) -> None:
        # Until convinced otherwise, we hate underscores since they look
        # like spaces and needlessly exacerbate our RSI.
        with self.assertRaisesRegex(RuntimeError, "fake_verse must match"):
            RpmTable("fake_verse")
