/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use anyhow::Result;
use std::sync::{Arc, RwLock};

use crate::Entity;

// Type-erasure trick. I don't actually care about T for RegisteredConfigEntity,
/// so hide it via a trait object
pub(crate) trait Refreshable {
    fn get_path(&self) -> &str;
    fn refresh(&self, entity: Entity) -> Result<bool>;
}

/// The type contained in a `ConfigHandle` when it's obtained from a `ConfigStore`
pub(crate) struct RegisteredConfigEntity<T> {
    contents: RwLock<CachedConfigEntity<T>>,
    path: String,
    deserializer: fn(String) -> Result<T>,
}

struct CachedConfigEntity<T> {
    mod_time: u64,
    version: Option<String>,
    contents: Arc<T>,
}

impl<T> RegisteredConfigEntity<T>
where
    T: Send + Sync + 'static,
{
    pub(crate) fn new(
        path: String,
        entity: Entity,
        deserializer: fn(String) -> Result<T>,
    ) -> Result<Self> {
        let Entity {
            mod_time,
            version,
            contents,
        } = entity;

        Ok(Self {
            contents: RwLock::new(CachedConfigEntity {
                mod_time,
                version,
                contents: Arc::new(deserializer(contents)?),
            }),
            path,
            deserializer,
        })
    }

    pub(crate) fn get(&self) -> Arc<T> {
        self.contents
            .read()
            .expect("lock poisoned")
            .contents
            .clone()
    }
}

impl<T> Refreshable for RegisteredConfigEntity<T>
where
    T: Send + Sync + 'static,
{
    fn get_path(&self) -> &str {
        &self.path
    }

    fn refresh(&self, entity: Entity) -> Result<bool> {
        let has_changed = {
            let locked = self.contents.read().expect("lock poisoned");
            entity.mod_time != locked.mod_time || entity.version != locked.version
        };

        if has_changed {
            let contents = Arc::new((self.deserializer)(entity.contents)?);
            {
                let mut locked = self.contents.write().expect("lock poisoned");
                *locked = CachedConfigEntity {
                    mod_time: entity.mod_time,
                    version: entity.version,
                    contents,
                };
                Ok(true)
            }
        } else {
            Ok(false)
        }
    }
}
