/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;

use serde::Serialize;
use snapshot_common::Checksums;

pub(crate) mod blob_status;
pub(crate) mod buck_files;
pub(crate) mod buck_target;
pub(crate) mod metadata;
pub(crate) mod packages;
pub(crate) mod progress;
pub(crate) mod snapshot;
pub(crate) mod storage;

#[derive(Debug, Serialize)]
pub(crate) struct Out {
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) checksums: BTreeMap<String, Checksums>,
}
