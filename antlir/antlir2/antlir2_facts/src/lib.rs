/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// stop warnings from typetag
#![allow(non_local_definitions)]

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::OptionalExtension;
use rusqlite::Row;
use rusqlite::Rows;
use serde::de::DeserializeOwned;

pub mod fact;
pub use antlir2_facts_macro::fact_impl;
pub use fact::Fact;
pub mod update_db;

use crate::fact::FactKind;
extern crate self as antlir2_facts;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("failure to populate facts db from layer: {0}")]
    Populate(anyhow::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Database<const RW: bool = false> {
    db: Connection,
}

pub type RwDatabase = Database<true>;
pub type RoDatabase = Database<false>;

impl RwDatabase {
    pub fn open<P>(path: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        Self::rw_db_pragmas(&db)?;
        Ok(Self { db })
    }

    /// Create a new empty database.
    pub fn create<P>(path: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = Connection::open(path)?;
        Self::rw_db_pragmas(&db)?;
        Self::setup_new_db(&db)?;
        Ok(Self { db })
    }

    fn rw_db_pragmas(db: &Connection) -> Result<()> {
        db.pragma_update(None, "foreign_keys", "ON")?;
        // If any program writing to this database even exits non-zero, buck2
        // will throw away the database, so we can use journal_mode=MEMORY and
        // lose any in-progress writes. This should prevent the database from
        // ever entering any kind of recovery mode.
        db.pragma_update(None, "journal_mode", "MEMORY")?;
        // Likewise, use temp_store=MEMORY to avoid creating any temporary files
        // on disk that could be lost during RE upload
        db.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(())
    }

    fn setup_new_db(db: &Connection) -> Result<()> {
        db.execute(
            "CREATE TABLE IF NOT EXISTS facts (kind TEXT, key BLOB, value TEXT, PRIMARY KEY (kind, key))",
            (),
        )?;
        Ok(())
    }

    pub fn insert<'f, F>(&mut self, fact: &'f F) -> Result<()>
    where
        F: Fact,
    {
        let val = serde_json::to_string(fact as &dyn Fact)?;
        self.db.execute(
            "INSERT OR REPLACE INTO facts (kind, key, value) VALUES (json_extract(?2, '$.type'), ?1, ?2)",
            (fact.key().as_ref(), val),
        )?;
        Ok(())
    }

    pub fn transaction(&mut self) -> Result<Transaction> {
        let tx = self.db.transaction()?;
        Ok(Transaction { tx })
    }

    pub fn to_readonly(self) -> Result<RoDatabase> {
        self.db.pragma_update(None, "query_only", "1")?;
        Ok(RoDatabase { db: self.db })
    }
}

impl RoDatabase {
    pub fn open<P>(path: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self { db })
    }

    pub fn open_read_only<P>(path: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        Self::open(path)
    }

    pub fn from_connection(connection: Connection) -> Self {
        Self { db: connection }
    }
}

// Exposed for antlir2_depgraph, but should be made private once again after
// fully merging the data structures used
#[doc(hidden)]
pub fn get_with_connection<F>(db: &Connection, key: impl Into<Key>) -> Result<Option<F>>
where
    F: Fact + FactKind + DeserializeOwned,
{
    let key: Key = key.into();
    let mut stmt = db.prepare("SELECT value FROM facts WHERE kind=? AND key=?")?;
    stmt.query_row((F::KIND, key.as_ref()), row_to_fact)
        .optional()
        .map_err(Error::from)
}

impl<const RW: bool> Database<{ RW }> {
    pub fn get<F>(&self, key: impl Into<Key>) -> Result<Option<F>>
    where
        F: Fact + FactKind + DeserializeOwned,
    {
        let key: Key = key.into();
        let mut stmt = self
            .db
            .prepare("SELECT value FROM facts WHERE kind=? AND key=?")?;
        stmt.query_row((F::KIND, key.as_ref()), row_to_fact)
            .optional()
            .map_err(Error::from)
    }

    // Iterate over all facts of a given type.
    pub fn iter<F>(&self) -> Result<FactIter<F>>
    where
        F: Fact + FactKind + DeserializeOwned,
    {
        // The lifetimes here are pretty hairy, so this eagerly loads all
        // matching keys. Most use cases require iterating over the entire Fact
        // space anyway, so this isn't that bad. `iter_prefix` offers the
        // ability to collect a narrower set of Facts.
        let mut stmt = self
            .db
            .prepare("SELECT value FROM facts WHERE kind=? ORDER BY key ASC")?;
        let rows = stmt.query((F::KIND,))?;
        let facts = rows_to_facts::<F>(rows)?;
        Ok(FactIter {
            iter: facts.into_iter(),
        })
    }

    /// Iterate over facts of this type with a given prefix. For example, you
    /// can iterate over [fact::DirEntry]s under a certain path.
    pub fn iter_prefix<F>(&self, key: &Key) -> Result<FactIter<F>>
    where
        F: Fact + FactKind + DeserializeOwned,
    {
        // The lifetimes here are pretty hairy, so this eagerly loads all
        // matching keys. The 'clone' feature is really the only use case that
        // has an early exit condition, so a follow-up diff will add some
        // functionality here to remain performant.

        // Iterate until we see `key` followed by 0xff (max value of a byte)
        // which ensures that we only return facts with a specific prefix.
        let mut end = key.as_ref().to_vec();
        end.push(0xff);

        let mut stmt = self.db.prepare(
            "SELECT value FROM facts WHERE kind=? AND key>=? AND key<? ORDER BY key ASC",
        )?;
        let rows = stmt.query((F::KIND, key.as_ref(), end.as_slice()))?;
        let facts = rows_to_facts::<F>(rows)?;
        Ok(FactIter {
            iter: facts.into_iter(),
        })
    }

    pub fn all_keys<F>(&self) -> Result<KeyIter>
    where
        F: Fact + FactKind,
    {
        // The lifetimes of querying are nasty, so just eagerly load all the
        // keys.
        let mut stmt = self.db.prepare("SELECT key FROM facts WHERE kind=?")?;
        let keys: Vec<Key> = stmt
            .query_map((F::KIND,), |row| row.get(0).map(Key))?
            .map(|res| res.map_err(Error::from))
            .collect::<Result<_>>()?;
        Ok(KeyIter(keys.into_iter()))
    }
}

impl<const RW: bool> AsRef<Connection> for Database<{ RW }> {
    fn as_ref(&self) -> &Connection {
        &self.db
    }
}

impl AsMut<Connection> for RwDatabase {
    fn as_mut(&mut self) -> &mut Connection {
        &mut self.db
    }
}

/// Transaction to write a batch of data into the db at once. Caller must call
/// [Transaction::commit] to preserve all the insertions, otherwise changes are
/// rolled back on drop.
pub struct Transaction<'db> {
    tx: rusqlite::Transaction<'db>,
}

impl<'db> Transaction<'db> {
    pub fn insert<'f, F>(&mut self, fact: &'f F) -> Result<()>
    where
        F: Fact,
    {
        let val = serde_json::to_string(fact as &dyn Fact)?;
        self.tx.execute(
            "INSERT OR REPLACE INTO facts (kind, key, value) VALUES (json_extract(?2, '$.type'), ?1, ?2)",
            (fact.key().as_ref(), val),
        )?;
        Ok(())
    }

    pub fn delete<F>(&mut self, key: &Key) -> Result<bool>
    where
        F: Fact + FactKind,
    {
        let num_rows = self.tx.execute(
            "DELETE FROM facts WHERE kind=? AND key=?",
            (F::KIND, key.as_ref()),
        )?;
        Ok(num_rows > 0)
    }

    pub fn all_keys<F>(&self) -> Result<KeyIter>
    where
        F: Fact + FactKind,
    {
        // The lifetimes of querying are nasty, so just eagerly load all the
        // keys.
        let mut stmt = self.tx.prepare("SELECT key FROM facts WHERE kind=?")?;
        let keys: Vec<Key> = stmt
            .query_map((F::KIND,), |row| row.get(0).map(Key))?
            .map(|res| res.map_err(Error::from))
            .collect::<Result<_>>()?;
        Ok(KeyIter(keys.into_iter()))
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

fn row_to_fact<F>(row: &Row) -> rusqlite::Result<F>
where
    F: for<'de> Fact + DeserializeOwned,
{
    // We can make this strong assertion about deserialization always
    // working because we know that the databases will only ever be read by
    // processes that are using the exact same code version as was used to
    // write them, since all antlir2 binaries are atomically built out of
    // (or possibly in the future, pinned into) the repo.
    let mut val: serde_json::Value = serde_json::from_str(row.get_ref("value")?.as_str()?)
        .expect("invalid JSON can never be stored in the DB");
    let inner = val
        .as_object_mut()
        .expect("invalid JSON can never be stored in the DB")
        .remove("value")
        .expect("invalid JSON can never be stored in the DB");
    Ok(serde_json::from_value(inner).expect("invalid JSON can never be stored in the DB"))
}

fn rows_to_facts<F>(rows: Rows) -> rusqlite::Result<Vec<F>>
where
    F: for<'de> Fact + DeserializeOwned,
{
    rows.mapped(row_to_fact).collect()
}

pub struct FactIter<F>
where
    F: for<'de> Fact,
{
    iter: <Vec<F> as IntoIterator>::IntoIter,
}

impl<F> Iterator for FactIter<F>
where
    F: Fact,
{
    type Item = F;

    fn next(&mut self) -> Option<F> {
        self.iter.next()
    }
}

pub struct KeyIter(<Vec<Key> as IntoIterator>::IntoIter);

impl Iterator for KeyIter {
    type Item = Key;

    fn next(&mut self) -> Option<Key> {
        self.0.next()
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Key(Vec<u8>);

impl AsRef<[u8]> for Key {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<'a> From<&'a [u8]> for Key {
    fn from(key: &'a [u8]) -> Self {
        Self(key.to_vec())
    }
}

impl<'a> From<&'a str> for Key {
    fn from(key: &'a str) -> Self {
        key.as_bytes().into()
    }
}

#[cfg(unix)]
impl<'a> From<&'a Path> for Key {
    fn from(key: &'a Path) -> Self {
        key.as_os_str().as_bytes().into()
    }
}

impl From<Vec<u8>> for Key {
    fn from(key: Vec<u8>) -> Self {
        Self(key)
    }
}

impl From<String> for Key {
    fn from(key: String) -> Self {
        key.into_bytes().into()
    }
}

#[cfg(unix)]
impl From<PathBuf> for Key {
    fn from(key: PathBuf) -> Self {
        key.into_os_string().into_vec().into()
    }
}

#[doc(hidden)]
pub mod __private {
    // another module so that we can just
    // use ::antlir2_facts::__private::typetag::*;
    // and know that it will always *only* import typetag
    #[doc(hidden)]
    pub mod typetag {
        pub use typetag;
    }
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    use super::*;
    use crate::fact::dir_entry::DirEntry;
    use crate::fact::dir_entry::FileCommon;
    use crate::fact::user::User;

    impl RwDatabase {
        fn new_test_db() -> Self {
            let db = Connection::open_in_memory().expect("failed to create in-mem db");
            Self::setup_new_db(&db).expect("failed to setup db");
            Self { db }
        }
    }

    #[test]
    #[traced_test]
    fn test_get() {
        let mut db = Database::new_test_db();

        db.insert(&User::new("alice", 1))
            .expect("failed to insert alice");
        assert_eq!(
            db.get::<User>(User::key("alice"))
                .expect("failed to get alice")
                .expect("alice not found")
                .name(),
            "alice"
        );
    }

    #[test]
    #[traced_test]
    fn test_iter() {
        let mut db = Database::new_test_db();

        db.insert(&User::new("alice", 1))
            .expect("failed to insert alice");

        let users: Vec<User> = db.iter().expect("failed to prepare iterator").collect();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name(), "alice");
    }

    #[test]
    #[traced_test]
    fn test_iter_prefix() {
        let mut db = Database::new_test_db();

        db.insert(&DirEntry::Directory(
            FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
        ))
        .expect("failed to insert /foo");
        db.insert(&DirEntry::RegularFile(
            FileCommon::new("/foo/bar".into(), 0, 0, 0o444).into(),
        ))
        .expect("failed to insert /foo/bar");
        db.insert(&DirEntry::Directory(
            FileCommon::new("/foo/baz".into(), 0, 0, 0o755).into(),
        ))
        .expect("failed to insert /foo/baz");
        db.insert(&DirEntry::RegularFile(
            FileCommon::new("/foo/baz/qux".into(), 0, 0, 0o444).into(),
        ))
        .expect("failed to insert /foo/baz/qux");
        db.insert(&DirEntry::RegularFile(
            FileCommon::new("/fooa".into(), 0, 0, 0o444).into(),
        ))
        .expect("failed to insert /fooa");
        // insert some other facts (that are lexicographically after these) to
        // make sure we don't iterate over them accidentally
        db.insert(&User::new("alice", 1))
            .expect("failed to insert alice");
        db.insert(&User::new("bob", 2))
            .expect("failed to insert bob");

        let entries: Vec<DirEntry> = db
            .iter_prefix(&DirEntry::key(Path::new("/foo/")))
            .expect("failed to prepare iterator")
            .collect();
        assert_eq!(entries.len(), 3, "{entries:?}");
        assert_eq!(entries[0].path(), Path::new("/foo/bar"));
        assert_eq!(entries[1].path(), Path::new("/foo/baz"));
        assert_eq!(entries[2].path(), Path::new("/foo/baz/qux"));
    }
}
