/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use itertools::Itertools;

#[derive(Debug)]
pub struct InstalledInfo {
    evra: String,
    size: u64,
    required_by: BTreeSet<String>,
}

impl InstalledInfo {
    pub fn evra(&self) -> &str {
        &self.evra
    }

    pub fn size(&self) -> &u64 {
        &self.size
    }

    pub fn required_by(&self) -> &BTreeSet<String> {
        &self.required_by
    }

    pub fn rpm_test_format(&self) -> String {
        let size_mb = self.size / (1024 * 1024);
        if self.required_by.is_empty() {
            return format!("{} MiB", size_mb);
        }

        format!(
            "{} MiB required by {}",
            size_mb,
            &self.required_by.iter().join(" ")
        )
    }
}

pub fn get_installed_rpms(layer: PathBuf) -> Result<BTreeMap<String, InstalledInfo>> {
    let res = Command::new("rpm")
        .arg("--root")
        .arg(&layer)
        .arg("-qa")
        .arg("--queryformat")
        .arg("%{NAME} %{EVR}.%{ARCH} %{SIZE}\\n")
        .output()
        .context("failed to execute rpm")?;

    ensure!(res.status.success(), "'rpm' failed");
    let mut installed = BTreeMap::<String, InstalledInfo>::new();
    for line in std::str::from_utf8(&res.stdout)
        .context("invalid utf8")?
        .lines()
    {
        let (name, evra, size) = line
            .split_whitespace()
            .collect_tuple()
            .with_context(|| format!("while parsing '{line}'"))?;
        let size: u64 = size
            .parse()
            .with_context(|| format!("size '{size}' is not an integer"))?;
        let res = Command::new("rpm")
            .arg("--root=/layer")
            .arg("-q")
            .arg("--whatrequires")
            .arg(name)
            .arg("--queryformat")
            .arg("%{NAME}\\n")
            .output()
            .context("failed to execute rpm")?;
        let stdout = std::str::from_utf8(&res.stdout).context("invalid utf8")?;
        let required_by = if !res.status.success() {
            ensure!(
                stdout.starts_with("no package requires"),
                "rpm query failed"
            );
            Default::default()
        } else {
            std::str::from_utf8(&res.stdout)
                .context("invalid utf8")?
                .lines()
                .map(str::to_owned)
                .collect()
        };

        installed.insert(
            name.to_owned(),
            InstalledInfo {
                evra: evra.to_owned(),
                size,
                required_by,
            },
        );
    }

    Ok(installed)
}
