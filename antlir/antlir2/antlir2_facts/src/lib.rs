/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;
use std::marker::PhantomData;

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

pub struct Database {
    #[cfg(fbrocks)]
    db: rocksdb::Db,
}

impl Database {
    #[cfg(fbrocks)]
    pub fn open<P>(path: P, options: rocksdb::Options) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let db = rocksdb::Db::open(path, options)?;
        Ok(Self { db })
    }
}

fn key_prefix<'a, 'de, F>() -> Vec<u8>
where
    F: Fact<'a, 'de>,
{
    let mut rocks_key = F::kind().as_bytes().to_vec();
    // strings can never have an internal nul-byte, so use that as a separator
    // that prevents incorrect iteration if one key type is a prefix of another
    // (eg 'User' and 'UserGroup')
    rocks_key.push(0);
    rocks_key
}

fn fact_key<'a, 'de, F>(fact: &'a F) -> Vec<u8>
where
    F: Fact<'a, 'de>,
{
    fact_key_to_rocks::<F>(fact.key().as_ref())
}

fn fact_key_to_rocks<'a, 'de, F>(key: &[u8]) -> Vec<u8>
where
    F: Fact<'a, 'de>,
{
    let mut rocks_key = key_prefix::<F>();
    rocks_key.extend_from_slice(key);
    rocks_key
}

impl Database {
    pub fn insert<'a, 'de, F>(&mut self, fact: &'a F) -> Result<()>
    where
        F: Fact<'a, 'de>,
    {
        let key = fact_key(fact);
        let write_opts = rocksdb::WriteOptions::new();
        self.db.put(key, serde_json::to_vec(fact)?, &write_opts)?;
        Ok(())
    }

    pub fn get<'a, F>(&self, key: <F as Fact<'a, '_>>::Key) -> Result<Option<F>>
    where
        F: for<'de> Fact<'a, 'de>,
    {
        let key = fact_key_to_rocks::<F>(key.as_ref());
        let read_opts = rocksdb::ReadOptions::new();
        match self.db.get(key, &read_opts)? {
            Some(value) => {
                // TODO: in theory rocksdb has a zero-copy api so we could get a
                // borrowed byte slice, but the crate doesn't actually expose
                // that, so we just make our own copy.
                Ok(Some(serde_json::from_reader(Cursor::new(&value))?))
            }
            None => Ok(None),
        }
    }

    // Iterate over all facts of a given type.
    pub fn iter<'a, F>(&self) -> FactIter<'a, F>
    where
        F: for<'de> Fact<'a, 'de>,
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
}

pub struct FactIter<'a, F>
where
    F: for<'de> Fact<'a, 'de>,
{
    iter: rocksdb::DbIterator,
    key_prefix: Vec<u8>,
    first: bool,
    phantom: PhantomData<&'a F>,
}

impl<'a, F> Iterator for FactIter<'a, F>
where
    F: for<'de> Fact<'a, 'de>,
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
        Some(serde_json::from_slice(value).expect("invalid JSON can never be stored in the DB"))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tracing_test::traced_test;

    use super::*;
    use crate::fact::user::User;

    impl Database {
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
    fn test_storage() {
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

        let users: Vec<User> = db.iter().collect();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name(), "alice");
    }
}
