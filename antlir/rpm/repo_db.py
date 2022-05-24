#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is the interface for reading and writing from the database that stores
the union of **all** the RPM repos that we ever snapshotted.  Each unique
RPM file and RPM repo database blob is stored just once, as a single row.

All tables in this database are append-only.

As implemented, the database is sufficient to reconstruct all snapshots that
were ever made.  One first pulls out a `repomd.xml` from the `repo_metadata`
table for a given `fetch_timestamp`, then one looks up repo database blobs
via the `repodata` table, and lastly one looks up RPM files via the `rpm`
table.  In this sense, the SQLite index linked from source control is 100%
redundant -- it primarily exists because many repo databases are XML, which
is horribly slow to parse at RPM fetch time.

All the query logic is in `RepoDBContext` below, so that's a good docblock
to read after this one.


## What is this "universe" column?

The top-level object that we typically snapshot is a `{dnf,yum}.conf` file,
which refers to multiple repos.

It is normal to have many different config files in production.

 - Two such files may use the same name to refer to completely **different**
   repos.  For example, the repo name "os" may refer to the universe
   "centos7" or "centos8".  Having the extra column allows us to store
   distinct `repomd.xml` files for these configs.

 - Conversely, the same "release-agnostic" repo may occur in two different
   config files, alongside with "centos7" and "centos8" repos mentioned
   above. We might put this repo in the "generic" universe.

From the above, you can see that the "universe" concept gives us the
flexibility to distinguish same-name repos that are different between config
files, or to share backing storage for repos that are the same.

Moreover, repos in different universes need not be related in any way.  So,
we cannot expect the same RPM NEVRAs to have the same contents across
universes.  For example, two universes might represent the same OS, but
built with incompatible compiler & linker flags.  For this reason,
"universe" namespaces not just the `repo_metadata` table, but also `rpm`.
The precise rules for mutable RPM errors are described in the doc for
`get_rpm_canonical_checksums` below.


## Bugs

  - If two RPM repos store the same RPM using different checksum algorithms,
    these will currently get two different storage IDs, wasting space in the
    key-value store.  This is rare enough I don't see it as worth fixing.


## Rationale: Why is it useful to have a DB?

Logically, this RPM repo snapshotter just wants to commit an atomic snapshot
of each RPM repo to this source control repo.  The physical reality is more
complex -- big binary blobs (RPMs and RPM databases) are stored in an
immutable key-value store via `storage/`, while we only commit a `Storage`
ID of a SQLite index of these blobs to source control, see `to_sqlite` in
`repo_snapshot.py`.

This extra indirection is worthwhile for two reasons:
  - Neither `hg` and `git` works well for writing large numbers of large
    binary blobs.  And scaling `svn` to a sufficient size is very expensive
    (compared with off-the-shelf key-value stores).
  - A key-value store also makes it trivial to scale up the read path,
    in other words -- fetching RPMs from historical repos. In contrast,
    source control check-outs would be neither efficient nor scalable.

Wanting indirection between source control and binary blobs does not
actually explain why a DB is useful. Couldn't we just store a pointer
to any snapshots in source control, and any large blobs in the key-value
store? Yes, we could.

A database adds value in two key ways:

  - Transactional writes make it much easier to correctly save successful
    work when a snapshot errors out partway.  Re-implementing the
    snapshotter's commit logic without a DB would be painful (and probably
    incorrect).

  - The DB enables several snapshotters to safely work in parallel.  This
    makes it trivial to parallelize the slow part of downloading all the
    RPMs.  The final index has to be assembled by a non-sharded snapshotter,
    so our typical sharding logic has this form:

        for shard in {0..10} ; do
            snapshot_repos $shard mod 100 &  # all write to DB, KV store
        done
        wait
        snapshot_repos 0 mod 1  # produce a complete snapshot
"""
import enum
import re
from contextlib import AbstractContextManager, contextmanager
from typing import ContextManager, FrozenSet, Iterator, Optional, Tuple, Union

from antlir.common import byteme

from .repo_objects import Checksum, Repodata, RepoMetadata, Rpm


# Deliberately excludes `-` since that is used by RPM filename parsing.
# Keeping `-` out of universe names allows `U-N-E:V-R.A` as a possible
# format, though we don't currently rely on that -- and probably should not,
# since it's 2020 and `json.dumps` / `repr` are cheap enough.
# pyre-fixme[5]: Global expression must be annotated.
_VALID_UNIVERSE_REGEX = re.compile("^[a-zA-Z0-9.]*$")


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def validate_universe_name(u):
    if not _VALID_UNIVERSE_REGEX.match(u):
        raise RuntimeError(
            f"Universe {u} must match {_VALID_UNIVERSE_REGEX.pattern}"
        )
    return u


class StorageTable:
    """
    A base class for correct `SELECT` / `INSERT` queries against all colums.
    Its child classes represent tables in our repo DB.
    """

    # pyre-fixme[3]: Return type must be annotated.
    def _column_funcs(self):
        "Ensures that column_{names,values} have the same order."
        # NB: We do not store `obj.location` in the DB, because the same
        # object can occur in different repos at different locations.
        return [
            ("checksum", lambda obj: str(obj.checksum)),
            ("build_timestamp", lambda obj: obj.build_timestamp),
            ("size", lambda obj: obj.size),
        ]

    # pyre-fixme[3]: Return type must be annotated.
    def column_names(self):
        return tuple(c for c, _ in self._column_funcs())

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def column_values(self, obj):
        return tuple(fn(obj) for _, fn in self._column_funcs())


class RpmTable(StorageTable):
    """
    Records all instances of RPM files that ever existed in our repos.

    The same RPM may occur in different repos, at different locations, so
    this table includes neither the repo, nor the location.  The doc for
    `_TABLE_KEYS` explains why we additionally key on `universe`.

    Moreover, repos may use different checksum algorithms for the same file.
    In that case, this table will include MULTIPLE ROWS with the same
    NEVRA -- in fact, the primary key is (<NEVRA>, `checksum`), which
    permits efficient retrieval of all checksums for a NEVRA.

    To detect different content hiding under the same NEVRA, we also
    store a "canonical_checksum".  Thus (<NEVRA>, `canonical_checksum`)
    must be unique for compliant RPMs -- any duplication signals that
    somebody changed the RPM content without changing the version. Since
    this is a snapshotting tool, we can only hope to detect, but not to
    prevent this. Thus, we do not have a `UNIQUE` constraint for this.
    """

    NAME = "rpm"
    KEY_COLUMNS = (
        "name",
        "epoch",
        "version",
        "release",
        "arch",
        "universe",
        "checksum",
    )

    # pyre-fixme[3]: Return type must be annotated.
    def __init__(self, universe: str):
        # pyre-fixme[4]: Attribute must be annotated.
        self._universe = validate_universe_name(universe)

    # pyre-fixme[3]: Return type must be annotated.
    def _column_funcs(self):
        return [
            ("name", lambda obj: obj.name),
            ("epoch", lambda obj: obj.epoch),
            ("version", lambda obj: obj.version),
            ("release", lambda obj: obj.release),
            ("arch", lambda obj: obj.arch),
            ("universe", lambda obj: self._universe),
            ("canonical_checksum", lambda obj: str(obj.canonical_checksum)),
            *super()._column_funcs(),
        ]

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def key(self, obj):  # Update KEY_COLUMNS, _TABLE_KEYS if changing this
        return (
            obj.name,
            obj.epoch,
            obj.version,
            obj.release,
            obj.arch,
            self._universe,
            str(obj.checksum),
        )


class RepodataTable(StorageTable):
    """
    Records all "repodata" blobs that ever existed in this repo (the SQLite
    or XML RPM primary indexes that are referenced from `repomd.xml`).

    Unlike `RpmTable`, it would not be too bad to include the repo name in
    this table.  However, in practice, one repo can be an alias of another,
    sharing the same repodata files.  By not referring to the repo name, we
    will only store such files once.
    """

    NAME = "repodata"
    KEY_COLUMNS = ("checksum",)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def key(self, obj):  # Update KEY_COLUMNS, _TABLE_KEYS if changing this
        return (str(obj.checksum),)


class SQLDialect(enum.Enum):
    SQLITE3 = "sqlite3"
    MYSQL = "mysql"


class RepoDBContext(AbstractContextManager):
    """
    A class to perform read & write queries against our DB of all historical
    RPM repo snapshots (as described in the top-level docblock).

    This is a context manager because a DB connection's useful lifetime is
    finite (i.e. MySQL ones will time out). Entering the context
    acquires a connection, then you run some queries, `commit()` if
    appropriate, and exit the context to release the connection.

    KEY INVARIANT: Write queries may only append new rows to tables.
    Existing rows must never be mutated.  With a lot of care, it would be
    possible to safely delete older rows (after verifying they are not
    referenced by newer snapshots) but this is not implemented for now.

    Notes:
      - Auto-commit is NOT enabled. You need to call `db_ctx.commit()` from
        inside this context.
      - These queries work in both SQLite (testing) and MySQL (production).
        The rationale is that automatically bringing up a test MySQL
        deployment is more work than supporting SQLite.
    """

    # pyre-fixme[4]: Attribute must be annotated.
    _DIALECT_TO_PLACEHOLDER = {SQLDialect.SQLITE3: "?", SQLDialect.MYSQL: "%s"}
    # pyre-fixme[4]: Attribute must be annotated.
    _DIALECT_TO_COLUMN_TYPE = {
        SQLDialect.SQLITE3: {
            "checksum": "TEXT",
            "canonical_checksum": "TEXT",
            "fetch_timestamp": "INTEGER",
            "build_timestamp": "INTEGER",
            "universe": "TEXT",
            "name": "TEXT",
            "epoch": "INTEGER",
            "version": "TEXT",
            "release": "TEXT",
            "arch": "TEXT",
            "repo": "TEXT",
            "size": "INTEGER",
            "storage_id": "TEXT",
            "xml": "BLOB",
        },
        SQLDialect.MYSQL: {
            "checksum": "VARCHAR(255)",  # Room for algo prefix + hex digest
            "canonical_checksum": "VARCHAR(255)",
            "fetch_timestamp": "BIGINT",  # THE EPOCHALYPSE IS NEAR
            "build_timestamp": "BIGINT",
            "universe": "VARCHAR(255)",  # Larger than is reasonable
            "name": "VARCHAR(255)",  # Linux max filename
            "epoch": "BIGINT",  # I've not seen an epoch above 99, but ...
            "version": "VARCHAR(255)",  # Linux max filename
            "release": "VARCHAR(255)",  # Linux max filename
            "arch": "VARCHAR(255)",  # Linux max filename
            "repo": "VARCHAR(255)",  # Linux max filename
            "size": "BIGINT",
            "storage_id": "VARCHAR(255)",  # Larger than is reasonable
            "xml": "BLOB",  # 64KB ought to be enough
        },
    }
    # pyre-fixme[4]: Attribute must be annotated.
    _TABLE_COLUMNS = {
        RpmTable.NAME: {
            "name": "NOT NULL",
            "epoch": "NOT NULL",
            "version": "NOT NULL",
            "release": "NOT NULL",
            "arch": "NOT NULL",
            "universe": "NOT NULL",  # See "universe" in the file docblock
            "checksum": "NOT NULL",  # As specified by the primary repodata
            "canonical_checksum": "NOT NULL",  # As computed by us
            "size": "NOT NULL",
            "build_timestamp": "NOT NULL",
            "storage_id": "NOT NULL",
        },
        RepodataTable.NAME: {
            "checksum": "NOT NULL",
            "size": "NOT NULL",
            "build_timestamp": "NOT NULL",
            "storage_id": "NOT NULL",
        },
        "repo_metadata": {
            "universe": "NOT NULL",  # See "universe" in the file docblock
            "repo": "NOT NULL",
            # The fetch time lets us reconstruct what all repos looked like
            # at a certain point in time. The build times may be much older.
            "fetch_timestamp": "NOT NULL",
            "build_timestamp": "NOT NULL",
            "checksum": "NOT NULL",
            "xml": "NOT NULL",
        },
    }
    # pyre-fixme[4]: Attribute must be annotated.
    _TABLE_KEYS = {
        # The key includes NEVRA because we try to detect when the same
        # NEVRA occurs with different contents in a set of repos.
        #
        # The key also includes a `universe` to allow completely unrelated
        # repo sets to use the same NEVRA to refer to different contents.
        #
        # `universe` follows NEVRA for a few reasons:
        #   - In `get_rpm_canonical_checksums`, we query a NEVRA among
        #     multiple universes, and this is efficient if NEVRA is first.
        #   - We do not commonly query for "all NEVRAs in a universe".  We
        #     also expect the number of universes to be small, constant, and
        #     comparable in size, so incurring a full-table scan for this
        #     scenario is not bad.
        #   - We expect NEVRA (especially `name`) to be far more
        #     discriminating than `universe`, so this improves point queries.
        #
        # `checksum` follows `universe` because it's long and is only useful
        # when we know exactly the row we want -- for our query patterns, it
        # is never not a "partial" disambiguator.
        #
        # Note that this strategy will store the same contents twice if it
        # occurs with different NEVRAs.  This should be rare, so we accept
        # the inefficiency to keep the code simpler.
        RpmTable.NAME: [
            "PRIMARY KEY (`name`, `epoch`, `version`, `release`, `arch`, "
            "`universe`, `checksum`)"
        ],
        # These don't need to be namespaced by `universe` because they are
        # only ever looked up by checksum.
        RepodataTable.NAME: ["PRIMARY KEY (`checksum`)"],
        # Unlike RepoTable, the top-level metadata is keyed on the repo
        # name, not just on checksum.  We need this to be able to
        # reconstruct the full repo state starting from a `{dnf,yum}.conf`.
        # Specifically, the config may have repo names that are aliases of
        # the same repo.  In that case, this table will store 2 copies of
        # the XML, which will resolve to the same rows in the repodata & rpm
        # tables.
        #
        # Keying on `universe` allows different `{yum,dnf}.conf` instances
        # to use the same name to refer to different repos.
        "repo_metadata": [
            # Includes `checksum` because it's technically possible for the
            # same `repomd.xml` to be fetched twice in the same second with
            # different results -- `test-snapshot-repos` hits this case.
            "PRIMARY KEY (`universe`, `repo`, `fetch_timestamp`, `checksum`)",
            # Don't store redundant copies, so if a repomd.xml stays
            # constant over a number of fetches, we only store the oldest.
            "UNIQUE (`universe`, `repo`, `checksum`)",
        ],
    }

    # pyre-fixme[3]: Return type must be annotated.
    def __init__(
        self,
        # This should be a multi-use context manager, which returns a
        # connection object, and optionally closes it on exit.  The goal is
        # to allow the caller to decide whether to return one persistent
        # connection, or to make a new one evert RepoDBContext.__enter__,
        conn_ctx: ContextManager[
            Union["sqlite3.Connection", "MySQLdb.Connection"]  # noqa: F821
        ],
        dialect: SQLDialect,
    ):
        self._conn_ctx = conn_ctx
        # pyre-fixme[4]: Attribute must be annotated.
        self._conn = None  # __enter__ this context to get a connection.
        self._dialect = dialect

    # pyre-fixme[3]: Return type must be annotated.
    def __enter__(self):
        assert self._conn is None, "RepoDBContext is not reentrant"
        self._conn = self._conn_ctx.__enter__()
        return self

    # pyre-fixme[2]: Parameter must be annotated.
    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        self._conn = None
        # pyre-fixme[7]: Expected `bool` but got `Optional[bool]`.
        return self._conn_ctx.__exit__(exc_type, exc_val, exc_tb)

    @contextmanager
    # pyre-fixme[3]: Return type must be annotated.
    def _cursor(self):
        cursor = self._conn.cursor()
        try:
            yield cursor
        finally:
            cursor.close()

    # pyre-fixme[3]: Return type must be annotated.
    def _placeholder(self):
        return self._DIALECT_TO_PLACEHOLDER[self._dialect]

    def _or_ignore(self) -> str:
        return f"{'' if self._dialect == SQLDialect.MYSQL else 'OR'} IGNORE"

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def _identifiers(self, identifiers):
        return ", ".join(f"`{i}`" for i in identifiers)

    # pyre-fixme[3]: Return type must be annotated.
    def ensure_tables_exist(self, _ensure_line_is_covered=lambda: None):
        # Future: it would be better if this function checked that the table
        # schemas in the DB are exactly as we would create them, and that
        # the DB's "canonical hash" algorithm matches ours.  For now, we'll
        # just trust that future developers will be careful to migrate the
        # DB correctly.
        with self._cursor() as cursor:
            col_types = self._DIALECT_TO_COLUMN_TYPE[self._dialect]
            for table, cols in self._TABLE_COLUMNS.items():
                # On MySQL, `IF NOT EXISTS` raises a warning, so `try` instead.
                try:
                    cursor.execute(
                        f"""
                        CREATE TABLE `{table}` (
                            {', '.join([
                                f'`{c}` {col_types[c]} {d}'
                                    for c, d in cols.items()
                            ] + list(self._TABLE_KEYS[table]))}
                        );
                    """
                    )
                except Exception as ex:
                    # The intent is to catch the 'aready exists' variants of
                    # {sqlite3,_mysql_exceptions}.OperationalError.  But, I
                    # don't want to import MySQLdb here, since it is an
                    # optional dependency for this specific module.
                    if (
                        type(ex).__qualname__ != "OperationalError"
                        or type(ex).__module__
                        not in [
                            "sqlite3",
                            # MySQLdb versions vary the module path.
                            "_mysql_exceptions",
                            "MySQLdb._exceptions",
                        ]
                        or "already exists" not in str(ex.args)
                    ):
                        raise  # pragma: no cover
                    _ensure_line_is_covered()

    def store_repomd(
        self, universe: str, repo: str, repomd: RepoMetadata
    ) -> int:
        "Returns the inserted `fetch_timestamp`, ours or from a racing writer"
        validate_universe_name(universe)
        with self._cursor() as cursor:
            fts = repomd.fetch_timestamp
            bts = repomd.build_timestamp
            checksum = str(repomd.checksum)
            repomd_xml = byteme(repomd.xml)

            # Future: We could start with a sanity check like below.  I'm
            # not sure of its value, though, and it would slow us down.
            #
            #     for repodata in repomd.repodatas:
            #         assert repodata.checksum() in DB
            p = self._placeholder()
            cursor.execute(
                f"""
                INSERT {self._or_ignore()} INTO `repo_metadata` (
                    `universe`, `repo`, `fetch_timestamp`,
                    `build_timestamp`, `checksum`, `xml`
                ) VALUES ({p}, {p}, {p}, {p}, {p}, {p});
            """,
                (universe, repo, fts, bts, checksum, repomd_xml),
            )
            if cursor.rowcount:
                return fts  # Our timestamp was the one that got inserted.

            # We lost the race, so ensure the prior data agrees with ours.
            # We don't need to check `build_timestamp`, it comes from `xml`.
            cursor.execute(
                f"""
                SELECT `fetch_timestamp`, `xml` FROM `repo_metadata`
                WHERE (`universe` = {p} AND `repo` = {p} AND `checksum` = {p});
            """,
                (universe, repo, checksum),
            )
            ((db_fts, db_repomd_xml),) = cursor.fetchall()
            # Allow a generous 1 minute of clock skew
            assert fts + 60 >= db_fts, f"{fts} + 60 < {db_fts}"
            assert repomd_xml == db_repomd_xml, f"{repomd_xml} {db_repomd_xml}"
            return db_fts

    def get_rpm_storage_id_and_checksum(
        self, tbl: RpmTable, rpm: Rpm
    ) -> Tuple[str, Checksum]:
        "Returns a storage_id, and its canonical checksum from the DB."
        assert rpm.canonical_checksum is None
        with self._cursor() as cursor:
            p = self._placeholder()
            # This query does not have a perfect index, but because our
            # primary key starts with NEVRA, it only has to iterate over all
            # the known checksums for a given NEVRA, which would at worst be
            # a handful.
            cursor.execute(
                f"""
                SELECT {self._identifiers(tbl.column_names())}, `storage_id`
                FROM `{tbl.NAME}`
                WHERE (
                    `name` = {p} AND `epoch` = {p} AND `version` = {p} AND
                    `release` = {p} AND `arch` = {p} AND `universe` = {p} AND (
                        `checksum` = {p} OR `canonical_checksum` = {p}
                    )
                )
            """,
                (
                    rpm.name,
                    rpm.epoch,
                    rpm.version,
                    rpm.release,
                    rpm.arch,
                    tbl._universe,
                    str(rpm.checksum),
                    str(rpm.checksum),
                ),
            )
            results = cursor.fetchall()
        if not results:
            # pyre-fixme[7]: Expected `Tuple[str, antlir.rpm.common.Checksum]`
            #  but got `Tuple[None, None]`.
            return None, None

        # We can get multiple results:
        #  - at most 1 match on the `checksum` column
        #  - many matches on the `canonical_checksum` column
        # However, they should all have the same size, canonical
        # checksum, and storage ID, so let's assert that.
        canonical_checksums = set()
        other_checksums = set()
        storage_ids = set()
        for db_values in results:
            storage_ids.add(db_values[-1])
            for col_name, db_val, val in zip(
                tbl.column_names(), db_values[:-1], tbl.column_values(rpm)
            ):
                if col_name == "checksum":
                    other_checksums.add(Checksum.from_string(db_val))
                elif col_name == "canonical_checksum":
                    canonical_checksums.add(Checksum.from_string(db_val))
                else:
                    assert db_val == val, f"{col_name} {db_val} {val}"
        assert len(storage_ids) == 1, storage_ids
        assert len(canonical_checksums) == 1, canonical_checksums
        assert rpm.checksum in (other_checksums | canonical_checksums)
        return (storage_ids.pop(), canonical_checksums.pop())

    def get_rpm_canonical_checksums_per_universe(
        self, table: RpmTable, rpm: Rpm, all_snapshot_universes: FrozenSet[str]
    ) -> Iterator[Tuple[Checksum, str]]:
        """
        Only the NEVRA part of `rpm` is used for the lookup.  We check for
        this NEVRA in all the universes involved in this snapshot, since any
        mutable RPM errors within the snapshot are bad, whether in the same
        universe, or in two different ones.  On the other hand, looking for
        mutable RPMs in completely unrelated universes does not make sense
        -- one design principle of universes that unrelated ones can
        legitimately use the same NEVRA to refer to different contents.
        """
        p = self._placeholder()
        assert table._universe in all_snapshot_universes
        all_snapshot_universe_ps = ", ".join([p] * len(all_snapshot_universes))
        with self._cursor() as cursor:
            # Like in `get_rpm_storage_id_and_checksums`, the primary
            # key helps make this query efficient.
            cursor.execute(
                f"""
                SELECT `canonical_checksum`, `universe` FROM `{table.NAME}`
                WHERE (
                    `name` = {p} AND `epoch` = {p} AND `version` = {p} AND
                    `release` = {p} AND `arch` = {p} AND
                    `universe` IN ({all_snapshot_universe_ps})
                )
            """,
                (
                    rpm.name,
                    rpm.epoch,
                    rpm.version,
                    rpm.release,
                    rpm.arch,
                    *all_snapshot_universes,
                ),
            )
            for (canonical_checksum, universe) in cursor.fetchall():
                yield (Checksum.from_string(canonical_checksum), universe)

    def get_storage_id(
        self, table: StorageTable, obj: Union["Rpm", Repodata]
    ) -> Optional[str]:
        with self._cursor() as cursor:
            # pyre-fixme[16]: `StorageTable` has no attribute `NAME`.
            table_name = table.NAME
            cursor.execute(
                # pyre-fixme[16]: `StorageTable` has no attribute `KEY_COLUMNS`.
                f"""
                SELECT {self._identifiers(table.column_names())}, `storage_id`
                FROM `{table_name}`
                WHERE ({
                    ' AND '.join(
                        f'`{c}` = {self._placeholder()}'
                            for c in table.KEY_COLUMNS
                    )
                })
            """,
                # pyre-fixme[16]: `StorageTable` has no attribute `key`.
                table.key(obj),
            )
            results = cursor.fetchall()
            if not results:
                return None
            (db_values,) = results
            # Check that the DB columns we got back agree with `obj`.
            for col_name, db_val, val in zip(
                table.column_names(), db_values[:-1], table.column_values(obj)
            ):
                # This `if` is explained in the `Repodata.build_timestamp`
                # doc.  In essence, we could have seen the same repodata
                # from a `repomd.xml` that was built either earlier or later
                # than the one already in the DB.
                if not (
                    col_name == "build_timestamp" and type(obj) is Repodata
                ):
                    assert db_val == val, (db_val, val, obj)
            return db_values[-1]  # We put `storage_id` last

    # pyre-fixme[2]: Parameter must be annotated.
    def maybe_store(self, table: StorageTable, obj, storage_id: str) -> str:
        """
        Records `obj` with `storage_id` in the DB, or if `obj` had already
        been stored, returns its pre-existing storage ID.
        """
        # If this were None, `INSERT OR IGNORE` would quietly return a row
        # count of 0, ultimately causing this function to return None.
        assert storage_id is not None
        # In theory, when this is storing the primary repodata, we could do
        # a consistency check asserting that all its RPMs are in the DB, but
        # this would make the transaction slow, and thus isn't worth it.
        with self._cursor() as cursor:
            col_names = table.column_names()
            # pyre-fixme[16]: `StorageTable` has no attribute `NAME`.
            table_name = table.NAME
            cursor.execute(
                f"""
                INSERT {self._or_ignore()} INTO `{table_name}`
                ({self._identifiers(col_names)}, `storage_id`)
                VALUES ({
                    ', '.join([self._placeholder()] * (len(col_names) + 1))
                })
            """,
                (*table.column_values(obj), storage_id),
            )
            if cursor.rowcount:
                return storage_id  # We won the race to insert our storage_id
            # Our storage_id will not be used, find the already-stored one.
            # pyre-fixme[7]: Expected `str` but got `Optional[str]`.
            return self.get_storage_id(table, obj)

    # pyre-fixme[3]: Return type must be annotated.
    def commit(self):
        self._conn.commit()
