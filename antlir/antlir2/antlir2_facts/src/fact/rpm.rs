/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;

use bon::Builder;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;

use super::Fact;
use super::Key;
use crate::fact_impl;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[builder(on(String, into))]
pub struct Rpm {
    name: String,
    #[serde(default, skip_serializing_if = "skip_epoch")]
    #[builder(default)]
    epoch: u64,
    version: String,
    release: String,
    arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    changelog: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    os: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    source_rpm: Option<String>,
}

fn skip_epoch(epoch: &u64) -> bool {
    *epoch == 0
}

#[fact_impl("antlir2_facts::fact::rpm::Rpm")]
impl Fact for Rpm {
    fn key(&self) -> Key {
        // It would be great to just use the name as the key, but a small set of
        // rpms can have multiple concurrently-installed versions, so just give
        // the full nevra as the key.
        self.nevra().into()
    }
}

static CVE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\bCVE-[0-9]{4}-[0-9]+\b").expect("valid regex"));

impl Rpm {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn release(&self) -> &str {
        &self.release
    }

    pub fn arch(&self) -> &str {
        &self.arch
    }

    pub fn nevra(&self) -> String {
        self.to_string()
    }

    pub fn changelog(&self) -> Option<&str> {
        self.changelog.as_deref()
    }

    pub fn patched_cves(&self) -> BTreeSet<&str> {
        self.changelog().map_or_else(Default::default, |changelog| {
            CVE_REGEX
                .find_iter(changelog)
                .map(|cve| cve.as_str())
                .collect()
        })
    }

    pub fn os(&self) -> Option<&str> {
        self.os.as_deref()
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn source_rpm(&self) -> Option<&str> {
        self.source_rpm.as_deref()
    }

    pub fn evra(&self) -> String {
        match self.epoch {
            0 => format!("{}-{}.{}", self.version, self.release, self.arch),
            epoch => format!("{}:{}-{}.{}", epoch, self.version, self.release, self.arch),
        }
    }
}

impl std::fmt::Display for Rpm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.epoch {
            0 => write!(
                f,
                "{}:{}-{}.{}",
                self.name, self.version, self.release, self.arch
            ),
            epoch => write!(
                f,
                "{}-{}:{}-{}.{}",
                self.name, epoch, self.version, self.release, self.arch
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cve_extraction() {
        let rpm = Rpm::builder()
            .name("foo")
            .version("1.2.3")
            .release("4")
            .arch("x86_64")
            .changelog("- CVE-2024-1234")
            .source_rpm("foo.src.rpm")
            .build();
        assert_eq!(rpm.patched_cves(), BTreeSet::from(["CVE-2024-1234"]));
    }
}
