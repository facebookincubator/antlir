/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::Serialize;

use crate::Object;
use crate::Result;

/// All [Object]s are uniquely identified by a [Checksum]
pub struct Checksum<T: Object> {
    hash: blake3::Hash,
    _phantom: PhantomData<fn() -> T>,
}

// Manual Copy/Clone impls to avoid unnecessary `T: Copy/Clone` bounds
// that #[derive] would add. The fields (blake3::Hash and PhantomData) are
// unconditionally Copy regardless of T.
impl<T: Object> Copy for Checksum<T> {}

impl<T: Object> Clone for Checksum<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Object> PartialEq<Checksum<T>> for Checksum<T> {
    fn eq(&self, other: &Checksum<T>) -> bool {
        self.hash == other.hash
    }
}

impl<T: Object> Eq for Checksum<T> {}

impl<T: Object> std::hash::Hash for Checksum<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl<T: Object> Checksum<T> {
    pub fn new(hash: blake3::Hash) -> Self {
        Self {
            hash,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn hex(&self) -> String {
        self.hash.to_hex().to_string()
    }
}

impl<T: Object> Debug for Checksum<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(&format!("Checksum<{}>", std::any::type_name::<T>()))
            .field(&self.hash.to_hex())
            .finish()
    }
}

impl<'de, T: Object> Deserialize<'de> for Checksum<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex = String::deserialize(deserializer)?;
        let hash = blake3::Hash::from_hex(hex)
            .map_err(|e| serde::de::Error::custom(format!("invalid hash: {e}")))?;
        Ok(Self {
            hash,
            _phantom: PhantomData,
        })
    }
}

impl<T: Object> Serialize for Checksum<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.hash.to_hex())
    }
}
