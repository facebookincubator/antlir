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

impl Package {
    /// Member identifier shared by yum and deb: `{name}_{version}_{arch}`
    /// built from the raw [`Package`] fields (yum `version` is the
    /// `EPOCH:VERSION-RELEASE` string, deb `version` the Debian version).
    /// For deb this matches the `.deb` basename convention (the `Filename`
    /// basename minus the `.deb` suffix). This is the value stored in the
    /// `members` attr of snapshotted `repo()`/`suite()` targets and used
    /// to key member artifacts. One format keeps the Starlark rule side
    /// and the apt/dnf consumers free of per-manager branching.
    pub fn nevra(&self) -> String {
        format!("{}_{}_{}", self.package, self.version, self.architecture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Checksums;

    fn test_package() -> Package {
        Package {
            package: "bash".to_owned(),
            version: "0:5.1.8-6.el9".to_owned(),
            architecture: "x86_64".to_owned(),
            filename: "Packages/bash-5.1.8-6.el9.x86_64.rpm".to_owned(),
            checksums: Checksums::new_sha256([0xAA; 32]),
        }
    }

    #[test]
    fn nevra_joins_name_version_arch() {
        assert_eq!(test_package().nevra(), "bash_0:5.1.8-6.el9_x86_64");
        let deb = Package {
            package: "0ad-data-common".to_owned(),
            version: "0.27.0-1".to_owned(),
            architecture: "all".to_owned(),
            filename: "pool/main/0/0ad-data/0ad-data-common_0.27.0-1_all.deb".to_owned(),
            checksums: Checksums::new_sha256([0xBB; 32]),
        };
        assert_eq!(deb.nevra(), "0ad-data-common_0.27.0-1_all");
    }
}
