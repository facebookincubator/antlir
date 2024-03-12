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

pub mod fact;
use fact::Fact;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(fbrocks)]
    #[error(transparent)]
    Rocksdb(#[from] rocksdb::RocksDBError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Database<const RW: bool = false> {
    #[cfg(fbrocks)]
    db: rocksdb::Db,
}

impl Database<true> {
    #[cfg(fbrocks)]
    pub fn open<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = rocksdb::Db::open(path, options)?;
        Ok(Self { db })
    }
}

impl Database<false> {
    #[cfg(fbrocks)]
    pub fn open<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = rocksdb::Db::open_for_read_only(path, options, false)?;
        Ok(Self { db })
    }

    #[cfg(fbrocks)]
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
        let write_opts = rocksdb::WriteOptions::new();
        let fact: &dyn Fact = fact;
        self.db.put(key, serde_json::to_vec(fact)?, &write_opts)?;
        Ok(())
    }
}

impl<const RW: bool> Database<{ RW }> {
    pub fn get<F>(&self, key: impl Into<Key>) -> Result<Option<F>>
    where
        F: Fact,
    {
        let key = fact_key_to_rocks::<F>(key.into().as_ref());
        let read_opts = rocksdb::ReadOptions::new();
        match self.db.get(key, &read_opts)? {
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
        let read_opts = rocksdb::ReadOptions::new();
        let key_prefix = key_prefix::<F>();
        let iter = self.db.iterator(
            rocksdb::IteratorMode::From(&key_prefix, rocksdb::Direction::Forward),
            &read_opts,
        );
        FactIter {
            iter,
            key_prefix,
            first: true,
            phantom: PhantomData,
        }
    }

    pub fn iter_from<F>(&self, key: &Key) -> FactIter<F>
    where
        F: Fact,
    {
        let read_opts = rocksdb::ReadOptions::new();
        let start_key = fact_key_to_rocks::<F>(key.as_ref());
        let iter = self.db.iterator(
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
            &read_opts,
        );
        FactIter {
            iter,
            key_prefix: key_prefix::<F>(),
            first: true,
            phantom: PhantomData,
        }
    }
}

pub struct FactIter<F>
where
    F: for<'de> Fact,
{
    iter: rocksdb::DbIterator,
    key_prefix: Vec<u8>,
    first: bool,
    phantom: PhantomData<F>,
}

impl<F> Iterator for FactIter<F>
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
                Self::open(
                    tmpdir.path().join(name),
                    rocksdb::Options::new().create_if_missing(true),
                )
                .expect("failed to open db"),
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
