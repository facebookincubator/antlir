/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeSet;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use typed_builder::TypedBuilder;

use super::Fact;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TypedBuilder)]
pub struct Rpm<'a> {
    #[builder(setter(into))]
    name: Cow<'a, str>,
    #[serde(default, skip_serializing_if = "skip_epoch")]
    #[builder(default)]
    epoch: u64,
    #[builder(setter(into))]
    version: Cow<'a, str>,
    #[builder(setter(into))]
    release: Cow<'a, str>,
    #[builder(setter(into))]
    arch: Cow<'a, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    changelog: Option<Cow<'a, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    os: Option<Cow<'a, str>>,
    #[builder(default)]
    size: u64,
    #[builder(setter(into))]
    source_rpm: Cow<'a, str>,
}

fn skip_epoch(epoch: &u64) -> bool {
    *epoch == 0
}

impl<'a> Fact<'a, '_> for Rpm<'a> {
    type Key = String;

    fn key(&'a self) -> Self::Key {
        // It would be great to just use the name as the key, but a small set of
        // rpms can have multiple concurrently-installed versions, so just give
        // the full nevra as the key.
        self.nevra()
    }
}

static CVE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\bCVE-[0-9]{4}-[0-9]+\b").expect("valid regex"));

impl<'a> Rpm<'a> {
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

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn source_rpm(&self) -> &str {
        &self.source_rpm
    }
}

impl<'a> std::fmt::Display for Rpm<'a> {
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
            .changelog(Some("- CVE-2024-1234".into()))
            .source_rpm("foo.src.rpm")
            .build();
        assert_eq!(rpm.patched_cves(), BTreeSet::from(["CVE-2024-1234"]));
    }
}