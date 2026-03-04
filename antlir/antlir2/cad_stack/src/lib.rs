/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod checksum;
mod object;
mod object_store;

pub use checksum::Checksum;
pub use object::Object;
pub use object_store::ObjectStore;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no such object '{0}'")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[doc(hidden)]
pub mod __deps {
    pub use serde_json;
}
