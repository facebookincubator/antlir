/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;

use cap_std::fs::Dir;
use nonempty::NonEmpty;

use crate::Checksum;
use crate::Error;
use crate::Result;
use crate::object::Object;

/// Storage backend for [Object]s.
/// Built to be reasonably efficient for the way buck2/RE stores output files.
/// A stack of directories is used such that all writes of new objects go to the
/// top directory while lookups can go all the way down the stack and simply
/// return at the first match.
pub struct ObjectStore {
    stack: NonEmpty<Dir>,
}

impl ObjectStore {
    pub fn new_from_empty<P>(top: P) -> std::io::Result<Self>
    where
        P: AsRef<OsStr>,
    {
        Self::open_rw(top, std::iter::empty::<&OsStr>())
    }

    pub fn open_rw<I, S, P>(top: P, stack: I) -> std::io::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        P: AsRef<OsStr>,
    {
        Dir::create_ambient_dir_all(top.as_ref(), cap_std::ambient_authority())?;
        let top = Dir::open_ambient_dir(top.as_ref(), cap_std::ambient_authority())?;
        let stack: Vec<_> = stack
            .into_iter()
            .map(|s| Dir::open_ambient_dir(s.as_ref(), cap_std::ambient_authority()))
            .collect::<std::io::Result<_>>()?;
        Ok(Self {
            stack: (top, stack).into(),
        })
    }

    pub fn exists<O: Object>(&self, checksum: &Checksum<O>) -> Result<bool> {
        self.exists_key(&checksum.hex())
    }

    fn exists_key(&self, key: &str) -> Result<bool> {
        for dir in self.stack.iter() {
            match dir.try_exists(key)? {
                true => return Ok(true),
                false => continue,
            }
        }
        Ok(false)
    }

    pub fn load<O: Object>(&self, checksum: &Checksum<O>) -> Result<O> {
        let key = checksum.hex();
        for dir in self.stack.iter() {
            match dir.open(&key) {
                Ok(f) => return O::from_file(f.into_std()),
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        continue;
                    }
                    return Err(Error::Io(e));
                }
            }
        }
        Err(Error::NotFound(key))
    }

    pub fn store<O: Object>(&self, object: &O) -> Result<Checksum<O>> {
        let checksum = object.checksum()?;
        let key = checksum.hex();
        // objects don't ever get removed from the stack, so if it exists, we
        // don't need to copy it into the current layer
        if self.exists_key(&key)? {
            return Ok(checksum);
        }
        let tmp_key = format!(".tmp.{key}");
        let top = self.stack.first();
        let mut file = top.create(&tmp_key)?.into_std();
        object.to_file(&mut file)?;
        file.sync_data()?;
        top.rename(&tmp_key, top, &key)?;
        Ok(checksum)
    }

    /// Add an alias to a specific object identified by [Checksum]. Aliases
    /// follow the same stack semantics in that later layers can override an
    /// alias from a previous one. Aliases cannot be deleted at this time.
    pub fn set_alias<O: Object>(&self, alias: &str, checksum: &Checksum<O>) -> Result<()> {
        let key = checksum.hex();
        if !self.exists_key(&key)? {
            return Err(Error::NotFound(key));
        }
        let top = self.stack.first();
        top.write(format!("alias.{alias}"), &key)?;
        Ok(())
    }

    /// Check the checksum of an object aliased to this name. Returns
    /// [Error::NotFound] if there is no such alias.
    pub fn get_alias_checksum<O: Object>(&self, alias: &str) -> Result<Checksum<O>> {
        let filename = format!("alias.{alias}");
        for dir in self.stack.iter() {
            match dir.read_to_string(&filename) {
                Ok(hex) => {
                    let hash = blake3::Hash::from_hex(hex)
                        .expect("aliases are valid checksums if they are stored by this crate");
                    return Ok(Checksum::new(hash));
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        continue;
                    }
                    return Err(Error::Io(e));
                }
            }
        }
        Err(Error::NotFound(alias.to_owned()))
    }

    /// Load the object aliased to this name
    pub fn load_by_alias<O: Object>(&self, alias: &str) -> Result<O> {
        let checksum = self.get_alias_checksum::<O>(alias)?;
        self.load(&checksum)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
    struct ExampleObject {
        foo: u32,
    }
    crate::json_object!(ExampleObject);

    #[test]
    fn test_round_trip_top_only() {
        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        let store = ObjectStore::new_from_empty(tmpdir.path().join("top"))
            .expect("failed to create ObjectStore");
        let obj = ExampleObject { foo: 42 };
        let csum = store.store(&obj).expect("failed to store object");
        let retrieved = store.load(&csum).expect("failed to retrieve meta object");
        assert_eq!(retrieved, obj, "stored and loaded object differ");
    }

    #[test]
    fn test_store_same_object_twice() {
        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        let store = ObjectStore::new_from_empty(tmpdir.path().join("bottom"))
            .expect("failed to create ObjectStore");
        let obj = ExampleObject { foo: 42 };
        let csum1 = store.store(&obj).expect("failed to store object");
        let csum2 = store.store(&obj).expect("failed to store object");
        assert_eq!(
            csum1, csum2,
            "storing the same object twice should be ok and return the same checksum"
        );

        let stacked_store = ObjectStore::open_rw(
            tmpdir.path().join("top"),
            std::iter::once(tmpdir.path().join("bottom")),
        )
        .expect("failed to create ObjectStore");
        let obj = ExampleObject { foo: 42 };
        let csum3 = stacked_store.store(&obj).expect("failed to store object");
        assert_eq!(
            csum3, csum1,
            "storing the same object twice should be ok and return the same checksum"
        );

        // and it shouldn't exist in the top layer at all - prove that by
        // removing the bottom layer and showing that it no longer exists
        let stacked_store = ObjectStore::new_from_empty(tmpdir.path().join("top"))
            .expect("failed to create ObjectStore");
        assert!(
            !stacked_store
                .exists(&csum3)
                .expect("failed to check existence"),
            "it shouldn't have been re-stored in the top layer"
        );
    }

    #[test]
    fn test_alias_round_trip_top_only() {
        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        let store = ObjectStore::new_from_empty(tmpdir.path().join("top"))
            .expect("failed to create ObjectStore");
        let obj = ExampleObject { foo: 42 };
        let csum = store.store(&obj).expect("failed to store object");
        store
            .set_alias("my_alias", &csum)
            .expect("failed to set alias");
        let retrieved: ExampleObject = store
            .load_by_alias("my_alias")
            .expect("failed to load by alias");
        assert_eq!(retrieved, obj, "alias should resolve to the stored object");
    }

    #[test]
    fn test_alias_shadow_same_name() {
        let tmpdir = TempDir::new().expect("failed to create tmpdir");

        // Store an object and alias it in the bottom layer
        let bottom_store = ObjectStore::new_from_empty(tmpdir.path().join("bottom"))
            .expect("failed to create ObjectStore");
        let obj1 = ExampleObject { foo: 1 };
        let csum1 = bottom_store.store(&obj1).expect("failed to store object");
        bottom_store
            .set_alias("name", &csum1)
            .expect("failed to set alias");

        // Create a stacked store and shadow the alias with a different object
        let stacked_store = ObjectStore::open_rw(
            tmpdir.path().join("top"),
            std::iter::once(tmpdir.path().join("bottom")),
        )
        .expect("failed to create ObjectStore");
        let obj2 = ExampleObject { foo: 2 };
        let csum2 = stacked_store.store(&obj2).expect("failed to store object");
        stacked_store
            .set_alias("name", &csum2)
            .expect("failed to set alias");

        // The alias should resolve to the top layer's object
        let retrieved: ExampleObject = stacked_store
            .load_by_alias("name")
            .expect("failed to load by alias");
        assert_eq!(
            retrieved, obj2,
            "alias should resolve to the top layer's object"
        );
    }

    #[test]
    fn test_alias_load_from_deeper_layer() {
        let tmpdir = TempDir::new().expect("failed to create tmpdir");

        // Store an object and alias it in the bottom layer
        let bottom_store = ObjectStore::new_from_empty(tmpdir.path().join("bottom"))
            .expect("failed to create ObjectStore");
        let obj = ExampleObject { foo: 99 };
        let csum = bottom_store.store(&obj).expect("failed to store object");
        bottom_store
            .set_alias("deep_alias", &csum)
            .expect("failed to set alias");

        // Create a stacked store with an empty top layer
        let stacked_store = ObjectStore::open_rw(
            tmpdir.path().join("top"),
            std::iter::once(tmpdir.path().join("bottom")),
        )
        .expect("failed to create ObjectStore");

        // Should find the alias from the bottom layer
        let retrieved: ExampleObject = stacked_store
            .load_by_alias("deep_alias")
            .expect("failed to load alias from deeper layer");
        assert_eq!(
            retrieved, obj,
            "alias from a deeper layer should still resolve"
        );
    }
}
