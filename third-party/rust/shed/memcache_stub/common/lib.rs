/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! This crate provides a client for accessing Memcache. The version on GitHub
//! is no-op for now.

#![deny(warnings, missing_docs, clippy::all, broken_intra_doc_links)]

mod client;
mod keygen;

pub use crate::client::{MemcacheClient, MemcacheGetType, MemcacheSetType};
pub use crate::keygen::KeyGen;

/// Memcache max size for key + value + overhead is around 1MB, so we are leaving 1KB for key +
/// overhead
pub const MEMCACHE_VALUE_MAX_SIZE: usize = 999_000;
