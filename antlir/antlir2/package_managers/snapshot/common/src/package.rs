/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::Checksums;

/// A single package entry in the snapshot pipeline.
///
/// Both deb and yum parsers produce this common representation so that
/// downstream commands (`blob_status`, `packages`) can work uniformly across
/// package managers. Each entry must have a top-level `filename` (used to
/// construct download URLs) and `checksums` (used for content-addressing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    /// The name of the package (e.g. "bash", "glibc").
    pub package: String,
    /// The package version string. For deb packages this is the full Debian
    /// version (may include epoch and revision). For RPM packages this is the
    /// EVR string formatted as `EPOCH:VERSION-RELEASE`.
    pub version: String,
    /// The machine architecture string (e.g. "amd64", "x86_64", "all",
    /// "noarch").
    pub architecture: String,
    /// The path to the package archive relative to the repository root.
    pub filename: String,
    /// Cryptographic hash(es) for verifying the package file integrity.
    pub checksums: Checksums,
}
