#!/usr/bin/env python3
'''
This is the interface for reading and writing from the database that stores
the union of **all** the RPM repos that we ever snapshotted.  Each unique
RPM file and RPM repo database blob is stored just once, as a single row.

All tables in this database are append-only.

As implemented, the database is sufficient to reconstruct all snapshots that
were ever made.  One first pulls out a `repomd.xml` from the `repo_metadata`
table for a given `fetch_timestamp`, then one looks up repo database blobs
via the `repodata` table, and lastly one looks up RPM files via the `rpm`
table.  In this sense, the JSON index in source control is 100% redundant --
it primarily exists because many repo databases are XML, which is horribly
slow to parse at RPM fetch time.  (NB: There's probably a better solution
than storing JSON in the repo -- e.g. storing a sqlite DB in our key-value
store would be easier to query and would not spam the repo).

All the query logic is in `RepoDBContext` below, so that's a good docblock
to read next.


## Bugs

  - If two RPM repos store the same RPM using different checksum algorithms,
    these will currently get two different storage IDs, wasting space in the
    key-value store.  This is rare enough I don't see it as worth fixing.


## Rationale: Why is it useful to have a DB?

Logically, this RPM repo snapshotter just wants to commit an atomic snapshot
of each RPM repo to this source control repo.  The physical reality is more
complex -- big binary blobs (RPMs and RPM databases) are stored in an
immutable key-value store via `storage/`, while we only commit a lightweight
JSON index of these blobs to source control, see `to_directory` in
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
        snapshot_repos 0 mod 1  # produce a complete snapshot, write JSON index


## Implementation detail: Why do you have `byteme` everywhere?

IMPORTANT: We must byte-coerce (`byteme`) all strings that go into the DB:

  - We store Linux paths, not airport novels, so the DBs should not be
    text-encoding aware, and so the column types are BLOB & VARBINARY.

  - MySQL then silently coerces strings to byte-strings:

    In [1]: import MySQLdb
       ...:
       ...: mconn = MySQLdb.connect(...)
       ...: mcur = mconn.cursor()
       ...: mcur.execute('CREATE TABLE IF NOT EXISTS `foo` (`bar` BLOB);')
       ...: mcur.execute('INSERT INTO `foo` (`bar`) VALUES (%s)', (b'bytes',))
       ...: mcur.execute('INSERT INTO `foo` (`bar`) VALUES (%s)', ('str',))
       ...: mcur.execute('SELECT `bar` FROM `foo`;')
       ...: mcur.fetchall(), mcur.rowcount
    Out[1]: (((b'bytes',), (b'str',)), 2)

  - SQLite stores byte-strings differently than unicode-strings:

    In [2]: import sqlite3
       ...:
       ...: sconn = sqlite3.connect(':memory:')
       ...: scur = sconn.cursor()
       ...: scur.execute('CREATE TABLE IF NOT EXISTS `foo` (`bar` BLOB);')
       ...: scur.execute('INSERT INTO `foo` (`bar`) VALUES (?)', (b'bytes',))
       ...: scur.execute('INSERT INTO `foo` (`bar`) VALUES (?)', ('str',))
       ...: scur.execute('SELECT `bar` FROM `foo`;')
       ...: scur.fetchall(), scur.rowcount
    Out[2]: ([(b'bytes',), ('str',)], -1)
'''
import enum

from contextlib import AbstractContextManager, contextmanager
from typing import AnyStr, ContextManager, Iterator, Optional, Tuple, Union


from .common import byteme
from .repo_objects import Checksum, Repodata, RepoMetadata, Rpm


def unbyteme(s: AnyStr) -> str:
    return s.decode() if isinstance(s, bytes) else s


class StorageTable:
    '''
    A base class for correct `SELECT` / `INSERT` queries against all colums.
    Its child classes represent tables in our repo DB.
    '''
    def _column_funcs(self):
        'Ensures that column_{names,values} have the same order.'
        # NB: We do not store `obj.location` in the DB, because the same
        # object can occur in different repos at different locations.
        return [
            ('checksum', lambda obj: byteme(str(obj.checksum))),
            ('build_timestamp', lambda obj: byteme(obj.build_timestamp)),
            ('size', lambda obj: byteme(obj.size)),
        ]

    def column_names(self):
        return tuple(c for c, _ in self._column_funcs())

    def column_values(self, obj):
        return tuple(fn(obj) for _, fn in self._column_funcs())


class RpmTable(StorageTable):
    '''
    Records all instances of RPM files that ever existed in our repos.

    The same RPM may occur in different repos, at different locations, so
    this table includes neither the repo, nor the location.

    Moreover, repos may use different checksum algorithms for the same file.
    In that case, this table will include MULTIPLE ROWS with the same
    filename -- in fact, the primary key is (`filename`, `checksum`), which
    permits efficient retrieval of all checksums for a filename.

    To detect different content hiding under the same filename, we also
    store a "canonical_checksum".  Thus (`filename`, `canonical_checksum`)
    must be unique for compliant RPMs -- any duplication signals that
    somebody changed the RPM content without changing the version. Since
    this is a snapshotting tool, we can only hope to detect, but not to
    prevent this. Thus, we do not have a `UNIQUE` constraint for this.
    '''
    NAME = 'rpm'
    KEY_COLUMNS = ('filename', 'checksum')
    CLASS = Rpm

    def _column_funcs(self):
        return [
            ('filename', lambda obj: byteme(obj.filename())),
            (
                'canonical_checksum',
                lambda obj: byteme(str(obj.canonical_checksum)),
            ),
            *super()._column_funcs(),
        ]

    def key(self, obj):  # Update KEY_COLUMNS, _TABLE_KEYS if changing this
        return (byteme(obj.filename()), byteme(str(obj.checksum)))


class RepodataTable(StorageTable):
    '''
    Records all "repodata" blobs that ever existed in this repo (the SQLite
    or XML RPM primary indexes that are referenced from `repomd.xml`).

    Unlike `RpmTable`, it would not be too bad to include the repo name in
    this table.  However, in practice, one repo can be an alias of another,
    sharing the same repodata files.  By not referring to the repo name, we
    will only store such files once.
    '''
    NAME = 'repodata'
    KEY_COLUMNS = ('checksum',)
    CLASS = Repodata

    def key(self, obj):  # Update KEY_COLUMNS, _TABLE_KEYS if changing this
        return (byteme(str(obj.checksum)),)


class SQLDialect(enum.Enum):
    SQLITE3 = 'sqlite3'
    MYSQL = 'mysql'


class RepoDBContext(AbstractContextManager):
    '''
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
    '''

    _DIALECT_TO_PLACEHOLDER = {SQLDialect.SQLITE3: '?', SQLDialect.MYSQL: '%s'}
    _DIALECT_TO_COLUMN_TYPE = {
        SQLDialect.SQLITE3: {
            'checksum': 'BLOB',
            'canonical_checksum': 'BLOB',
            'fetch_timestamp': 'INTEGER',
            'build_timestamp': 'INTEGER',
            'filename': 'BLOB',
            'repo': 'BLOB',
            'size': 'INTEGER',
            'storage_id': 'BLOB',
            'xml': 'BLOB',
        },
        # VARBINARY to avoid dealing with charsets.
        SQLDialect.MYSQL: {
            'checksum': 'VARBINARY(255)',  # Room for algo prefix + hex digest
            'canonical_checksum': 'VARBINARY(255)',
            'fetch_timestamp': 'BIGINT',  # THE EPOCHALYPSE IS NEAR
            'build_timestamp': 'BIGINT',
            'filename': 'VARBINARY(255)',  # Linux max filename
            'repo': 'VARBINARY(255)',  # Linux max filename
            'size': 'BIGINT',
            'storage_id': 'VARBINARY(255)',  # Larger than reasonable.
            'xml': 'BLOB',  # 64KB ought to be enough
        },
    }
    _TABLE_COLUMNS = {
        RpmTable.NAME: {
            'filename': 'NOT NULL',
            'checksum': 'NOT NULL',  # As specified by the primary repodata
            'canonical_checksum': 'NOT NULL',  # As computed by us
            'size': 'NOT NULL',
            'build_timestamp': 'NOT NULL',
            'storage_id': 'NOT NULL',
        },
        RepodataTable.NAME: {
            'checksum': 'NOT NULL',
            'size': 'NOT NULL',
            'build_timestamp': 'NOT NULL',
            'storage_id': 'NOT NULL',
        },
        'repo_metadata': {
            'repo': 'NOT NULL',
            # The fetch time lets us reconstruct what all repos looked like
            # at a certain point in time. The build times may be much older.
            'fetch_timestamp': 'NOT NULL',
            'build_timestamp': 'NOT NULL',
            'checksum': 'NOT NULL',
            'xml': 'NOT NULL',
        },
    }
    _TABLE_KEYS = {
        RpmTable.NAME: ['PRIMARY KEY (`filename`, `checksum`)'],
        RepodataTable.NAME: ['PRIMARY KEY (`checksum`)'],
        # Unlike RpmTable and RepoTable, the top-level metadata is keyed on
        # the repo name.  That lets us handle repos that are aliases of each
        # other without copying large repodata blobs.
        'repo_metadata': [
            'PRIMARY KEY (`repo`, `fetch_timestamp`)',
            # We don't store redundant copies, so if a repomd.xml stays
            # constant over a number of fetches, we only store the oldest.
            'UNIQUE (`repo`, `checksum`)',
        ],
    }

    def __init__(
        self,
        # This should be a multi-use context manager, which returns a
        # connection object, and optionally closes it on exit.  The goal is
        # to allow the caller to decide whether to return one persistent
        # connection, or to make a new one evert RepoDBContext.__enter__,
        conn_ctx: ContextManager[Union[
            'sqlite3.Connection', 'MySQLdb.Connection',
        ]],
        dialect: SQLDialect,
    ):
        self._conn_ctx = conn_ctx
        self._conn = None  # __enter__ this context to get a connection.
        self._dialect = dialect
        with self as db:
            db._ensure_tables_exist()

    def __enter__(self):
        assert self._conn is None, 'RepoDBContext is not reentrant'
        self._conn = self._conn_ctx.__enter__()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        self._conn = None
        return self._conn_ctx.__exit__(exc_type, exc_val, exc_tb)

    @contextmanager
    def _cursor(self):
        cursor = self._conn.cursor()
        try:
            yield cursor
        finally:
            cursor.close()

    def _placeholder(self):
        return self._DIALECT_TO_PLACEHOLDER[self._dialect]

    def _or_ignore(self) -> str:
        return f"{'' if self._dialect == SQLDialect.MYSQL else 'OR'} IGNORE"

    def _identifiers(self, identifiers):
        return ', '.join(f'`{i}`' for i in identifiers)

    def _ensure_tables_exist(self, _ensure_line_is_covered=lambda: None):
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
                    cursor.execute(f'''
                        CREATE TABLE `{table}` (
                            {', '.join([
                                f'`{c}` {col_types[c]} {d}'
                                    for c, d in cols.items()
                            ] + [k for k in self._TABLE_KEYS[table]])}
                        );
                    ''')
                except Exception as ex:
                    # The intent is to catch the 'aready exists' variants of
                    # {sqlite3,_mysql_exceptions}.OperationalError.  But, I
                    # don't want to import MySQLdb here, since it is an
                    # optional dependency for this specific module.
                    if (
                        type(ex).__qualname__ != 'OperationalError' or
                        type(ex).__module__ not in [
                            'sqlite3',
                            # MySQLdb versions vary the module path.
                            '_mysql_exceptions', 'MySQLdb._exceptions',
                        ] or
                        'already exists' not in str(ex.args)
                    ):
                        raise  # pragma: no cover
                    _ensure_line_is_covered()

    def store_repomd(self, repo_str: str, repomd: RepoMetadata) -> int:
        'Returns the inserted `fetch_timestamp`, ours or from a racing writer'
        with self._cursor() as cursor:
            # Prepare columns, see above "Why is `byteme` everywhere?"
            repo = byteme(repo_str)
            fts = repomd.fetch_timestamp
            bts = repomd.build_timestamp
            checksum = byteme(str(repomd.checksum))
            repomd_xml = byteme(repomd.xml)

            # Future: We could start with a sanity check like below.  I'm
            # not sure of its value, though, and it would slow us down.
            #
            #     for repodata in repomd.repodatas:
            #         assert repodata.checksum() in DB
            p = self._placeholder()
            cursor.execute(f'''
                INSERT {self._or_ignore()} INTO `repo_metadata` (
                `repo`, `fetch_timestamp`, `build_timestamp`, `checksum`, `xml`
                ) VALUES ({p}, {p}, {p}, {p}, {p});
            ''', (repo, fts, bts, checksum, repomd_xml))
            if cursor.rowcount:
                return fts  # Our timestamp was the one that got inserted.

            # We lost the race, so ensure the prior data agrees with ours.
            # We don't need to check `build_timestamp`, it comes from `xml`.
            cursor.execute(f'''
                SELECT `fetch_timestamp`, `xml` FROM `repo_metadata`
                WHERE (`repo` = {p} AND `checksum` = {p});
            ''', (repo, checksum))
            (db_fts, db_repomd_xml), = cursor.fetchall()
            # Allow a generous 1 minute of clock skew
            assert fts + 60 >= db_fts, f'{fts} + 60 < {db_fts}'
            assert repomd_xml == db_repomd_xml, f'{repomd_xml} {db_repomd_xml}'
            return db_fts

    def get_rpm_storage_id_and_checksum(self, tbl: RpmTable, rpm: Rpm) \
            -> Tuple[str, Checksum]:
        'Returns a storage_id, and its canonical checksum from the DB.'
        assert rpm.canonical_checksum is None
        with self._cursor() as cursor:
            p = self._placeholder()
            checksum = byteme(str(rpm.checksum))
            # This query does not have a perfect index, but because our
            # primary key starts with `filename`, it only has to iterate
            # over all the known checksums for a given filename, which would
            # at worst be a handful.
            cursor.execute(f'''
                SELECT {self._identifiers(tbl.column_names())}, `storage_id`
                FROM `{tbl.NAME}`
                WHERE (`filename` = {p} AND (
                    `checksum` = {p} OR `canonical_checksum` = {p}
                ))
            ''', (byteme(rpm.filename()), checksum, checksum))
            results = cursor.fetchall()
            if not results:
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
                storage_ids.add(db_values[-1].decode('latin-1'))
                for col_name, db_val, val in zip(
                    tbl.column_names(), db_values[:-1], tbl.column_values(rpm),
                ):
                    if col_name == 'checksum':
                        other_checksums.add(
                            Checksum.from_string(unbyteme(db_val))
                        )
                    elif col_name == 'canonical_checksum':
                        canonical_checksums.add(
                            Checksum.from_string(unbyteme(db_val))
                        )
                    else:
                        assert db_val == val, f'{col_name} {db_val} {val}'
            assert len(storage_ids) == 1, storage_ids
            assert len(canonical_checksums) == 1, canonical_checksums
            assert rpm.checksum in (other_checksums | canonical_checksums)
            return (storage_ids.pop(), canonical_checksums.pop())

    def get_rpm_canonical_checksums(self, table: RpmTable, filename: str) \
            -> Iterator[Checksum]:
        with self._cursor() as cursor:
            # Like in `get_rpm_storage_id_and_checksums`, the primary
            # key helps make this query efficient.
            cursor.execute(f'''
                SELECT `canonical_checksum` FROM `{table.NAME}`
                WHERE (`filename` = {self._placeholder()})
            ''', (byteme(filename),))
            for (canonical_checksum,) in cursor.fetchall():
                yield Checksum.from_string(canonical_checksum.decode())

    def get_storage_id(
        self, table: StorageTable, obj: Union['Rpm', Repodata],
    ) -> Optional[str]:
        with self._cursor() as cursor:
            cursor.execute(f'''
                SELECT {self._identifiers(table.column_names())}, `storage_id`
                FROM `{table.NAME}`
                WHERE ({
                    ' AND '.join(
                        f'`{c}` = {self._placeholder()}'
                            for c in table.KEY_COLUMNS
                    )
                })
            ''', table.key(obj))
            results = cursor.fetchall()
            if not results:
                return None
            db_values, = results
            # Check that the DB columns we got back agree with `obj`.
            for col_name, db_val, val in zip(
                table.column_names(), db_values[:-1], table.column_values(obj),
            ):
                # Explained in this field's declaration in `Repodata`
                if col_name == 'build_timestamp' and type(obj) is Repodata:
                    assert db_val <= val, (db_val, val, obj)
                else:
                    assert db_val == val, (db_val, val, obj)
            return db_values[-1].decode('latin-1')  # We put `storage_id` last

    def maybe_store(self, table: StorageTable, obj, storage_id: str) -> str:
        '''
        Records `obj` with `storage_id` in the DB, or if `obj` had already
        been stored, returns its pre-existing storage ID.
        '''
        # If this were None, `INSERT OR IGNORE` would quietly return a row
        # count of 0, ultimately causing this function to return None.
        assert storage_id is not None
        # In theory, when this is storing the primary repodata, we could do
        # a consistency check asserting that all its RPMs are in the DB, but
        # this would make the transaction slow, and thus isn't worth it.  We
        # can do this kind of expensive check before writing out a JSON view
        # of the current repo to the filesystem for version control.
        with self._cursor() as cursor:
            col_names = table.column_names()
            cursor.execute(f'''
                INSERT {self._or_ignore()} INTO `{table.NAME}`
                ({self._identifiers(col_names)}, `storage_id`)
                VALUES ({
                    ', '.join([self._placeholder()] * (len(col_names) + 1))
                })
            ''', (*table.column_values(obj), byteme(storage_id)))
            if cursor.rowcount:
                return storage_id  # We won the race to insert our storage_id
            # Our storage_id will not be used, find the already-stored one.
            return self.get_storage_id(table, obj)

    def commit(self):
        self._conn.commit()
