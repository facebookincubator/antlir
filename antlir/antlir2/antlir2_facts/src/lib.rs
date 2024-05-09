/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(trait_upcasting)]

use std::any::Any;
use std::marker::PhantomData;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;

#[cfg(fbrocks)]
use rocksdb::Db as RocksDb;
#[cfg(not(fbrocks))]
use rocksdb::Error as RocksDBError;
#[cfg(fbrocks)]
use rocksdb::RocksDBError;
#[cfg(not(fbrocks))]
use rocksdb::DB as RocksDb;

pub mod fact;
use fact::Fact;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Rocksdb(#[from] RocksDBError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Database<const RW: bool = false> {
    db: RocksDb,
}

impl Database<true> {
    pub fn open<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        #[cfg(fbrocks)]
        let db = RocksDb::open(path, options)?;
        #[cfg(not(fbrocks))]
        let db = RocksDb::open(&options, path)?;
        Ok(Self { db })
    }

    /// Create a new empty database.
    pub fn create<P>(path: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        Self::open(path, opts_for_new_db())
    }
}

impl Database<false> {
    pub fn open<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        #[cfg(fbrocks)]
        let db = RocksDb::open_for_read_only(path, options, false)?;
        #[cfg(not(fbrocks))]
        let db = RocksDb::open_for_read_only(&options, path, false)?;
        Ok(Self { db })
    }

    pub fn open_read_only<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        Self::open(path, options)
    }
}

pub type RwDatabase = Database<true>;
pub type RoDatabase = Database<false>;

fn key_prefix<F>() -> Vec<u8>
where
    F: Fact,
{
    let mut rocks_key = F::kind().as_bytes().to_vec();
    // strings can never have an internal nul-byte, so use that as a separator
    // that prevents incorrect iteration if one key type is a prefix of another
    // (eg 'User' and 'UserGroup')
    rocks_key.push(0);
    rocks_key
}

fn fact_key<F>(fact: &F) -> Vec<u8>
where
    F: Fact,
{
    fact_key_to_rocks::<F>(fact.key().as_ref())
}

fn fact_key_to_rocks<F>(key: &[u8]) -> Vec<u8>
where
    F: Fact,
{
    let mut rocks_key = key_prefix::<F>();
    rocks_key.extend_from_slice(key);
    rocks_key
}

impl Database<true> {
    pub fn insert<'a, F>(&mut self, fact: &'a F) -> Result<()>
    where
        F: Fact,
    {
        let key = fact_key(fact);
        let fact: &dyn Fact = fact;
        let val = serde_json::to_vec(fact)?;
        #[cfg(fbrocks)]
        {
            let write_opts = rocksdb::WriteOptions::new();
            self.db.put(key, val, &write_opts)?;
        }
        #[cfg(not(fbrocks))]
        self.db.put(key, val)?;

        Ok(())
    }
}

impl<const RW: bool> Database<{ RW }> {
    pub fn get<F>(&self, key: impl Into<Key>) -> Result<Option<F>>
    where
        F: Fact,
    {
        let key = fact_key_to_rocks::<F>(key.into().as_ref());
        #[cfg(fbrocks)]
        let get_res = self.db.get(key, &rocksdb::ReadOptions::new());
        #[cfg(not(fbrocks))]
        let get_res = self.db.get(key);
        match get_res? {
            Some(value) => {
                // TODO: in theory rocksdb has a zero-copy api so we could get a
                // borrowed byte slice, but the crate doesn't actually expose
                // that, so we just make our own copy.
                let fact: Box<dyn Fact> = serde_json::from_slice(value.as_ref())?;
                let fact: Box<dyn Any> = fact;
                let fact: Box<F> = fact
                    .downcast()
                    .expect("the type name is part of the key, so this should never fail");
                Ok(Some(*fact))
            }
            None => Ok(None),
        }
    }

    // Iterate over all facts of a given type.
    pub fn iter<F>(&self) -> FactIter<F>
    where
        F: Fact,
    {
        let key_prefix = key_prefix::<F>();
        #[cfg(fbrocks)]
        let iter = {
            let read_opts = rocksdb::ReadOptions::new();
            self.db.iterator(
                rocksdb::IteratorMode::From(&key_prefix, rocksdb::Direction::Forward),
                &read_opts,
            )
        };
        #[cfg(not(fbrocks))]
        let iter = self.db.iterator(rocksdb::IteratorMode::From(
            &key_prefix,
            rocksdb::Direction::Forward,
        ));
        FactIter {
            iter,
            key_prefix,
            #[cfg(fbrocks)]
            first: true,
            phantom: PhantomData,
        }
    }

    pub fn iter_from<F>(&self, key: &Key) -> FactIter<F>
    where
        F: Fact,
    {
        let start_key = fact_key_to_rocks::<F>(key.as_ref());
        let iter = self.db.iterator(
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
            #[cfg(fbrocks)]
            &rocksdb::ReadOptions::new(),
        );
        FactIter {
            iter,
            key_prefix: key_prefix::<F>(),
            #[cfg(fbrocks)]
            first: true,
            phantom: PhantomData,
        }
    }
}

pub struct FactIter<'a, F>
where
    F: for<'de> Fact,
{
    #[cfg(fbrocks)]
    iter: rocksdb::DbIterator,
    #[cfg(not(fbrocks))]
    iter: rocksdb::DBIterator<'a>,
    key_prefix: Vec<u8>,
    #[cfg(fbrocks)]
    first: bool,
    phantom: PhantomData<&'a F>,
}

#[cfg(fbrocks)]
impl<'a, F> Iterator for FactIter<'a, F>
where
    F: Fact,
{
    type Item = F;

    fn next(&mut self) -> Option<F> {
        if !self.first {
            // RocksDB iterators start pointing at the first item, so we need to
            // skip the iterator advance on the first iteration
            self.iter.next();
        }
        let (key, value) = self.iter.item()?;
        if (key.len() <= self.key_prefix.len())
            || (key[0..self.key_prefix.len()] != self.key_prefix[..])
        {
            return None;
        }
        self.first = false;
        // We can make this strong assertion about deserialization always
        // working because we know that the databases will only ever be read by
        // processes that are using the exact same code version as was used to
        // write them, since all antlir2 binaries are atomically built out of
        // (or possibly in the future, pinned into) the repo.
        let fact: Box<dyn Fact> =
            serde_json::from_slice(value).expect("invalid JSON can never be stored in the DB");
        let fact: Box<dyn Any> = fact;
        let fact: Box<F> = fact
            .downcast()
            .expect("the type name is part of the key, so this should never fail");
        Some(*fact)
    }
}

#[cfg(not(fbrocks))]
impl<'a, F> Iterator for FactIter<'a, F>
where
    F: Fact,
{
    type Item = F;

    fn next(&mut self) -> Option<F> {
        let (key, value) = self.iter.next()?.ok()?;
        if (key.len() <= self.key_prefix.len())
            || (key[0..self.key_prefix.len()] != self.key_prefix[..])
        {
            return None;
        }
        // We can make this strong assertion about deserialization always
        // working because we know that the databases will only ever be read by
        // processes that are using the exact same code version as was used to
        // write them, since all antlir2 binaries are atomically built out of
        // (or possibly in the future, pinned into) the repo.
        let fact: Box<dyn Fact> =
            serde_json::from_slice(&value).expect("invalid JSON can never be stored in the DB");
        let fact: Box<dyn Any> = fact;
        let fact: Box<F> = fact
            .downcast()
            .expect("the type name is part of the key, so this should never fail");
        Some(*fact)
    }
}

#[derive(Clone)]
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

#[cfg(fbrocks)]
fn opts_for_new_db() -> rocksdb::Options {
    let opts = rocksdb::Options::new();
    opts.create_if_missing(true)
}

#[cfg(not(fbrocks))]
fn opts_for_new_db() -> rocksdb::Options {
    let mut opts = rocksdb::Options::default();
    opts.create_if_missing(true);
    opts
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tracing_test::traced_test;

    use super::*;
    use crate::fact::user::User;

    impl RwDatabase {
        fn open_test_db(name: &str) -> (Self, TempDir) {
            let tmpdir = TempDir::new().expect("failed to create tempdir");
            (
                Self::create(tmpdir.path().join(name)).expect("failed to open db"),
                tmpdir,
            )
        }
    }

    #[test]
    #[traced_test]
    fn test_get() {
        let (mut db, _tmpdir) = Database::open_test_db("_test_storage");

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
        let (mut db, _tmpdir) = Database::open_test_db("_test_iter");

        db.insert(&User::new("alice", 1))
            .expect("failed to insert alice");

        let users: Vec<User> = db.iter().collect();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name(), "alice");
    }

    #[test]
    #[traced_test]
    fn test_iter_from() {
        let (mut db, _tmpdir) = Database::open_test_db("_test_iter_from");

        db.insert(&User::new("alice", 1))
            .expect("failed to insert alice");
        db.insert(&User::new("bob", 1))
            .expect("failed to insert bob");
        db.insert(&User::new("charlie", 1))
            .expect("failed to insert charlie");

        let users: Vec<User> = db.iter_from(&User::key("bob")).collect();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name(), "bob");
        assert_eq!(users[1].name(), "charlie");
    }
}
