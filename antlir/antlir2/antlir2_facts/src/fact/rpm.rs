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

use super::Fact;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rpm<'a> {
    name: Cow<'a, str>,
    #[serde(default, skip_serializing_if = "skip_epoch")]
    epoch: u64,
    version: Cow<'a, str>,
    release: Cow<'a, str>,
    arch: Cow<'a, str>,
    changelog: Option<Cow<'a, str>>,
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
    pub fn new<N, V, R, A, C>(
        name: N,
        epoch: u64,
        version: V,
        release: R,
        arch: A,
        changelog: Option<C>,
    ) -> Self
    where
        N: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
        R: Into<Cow<'a, str>>,
        A: Into<Cow<'a, str>>,
        C: Into<Cow<'a, str>>,
    {
        Self {
            name: name.into(),
            epoch,
            version: version.into(),
            release: release.into(),
            arch: arch.into(),
            changelog: changelog.map(|c| c.into()),
        }
    }

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
        let rpm = Rpm::new("foo", 0, "1.2.3", "4", "x86_64", Some("- CVE-2024-1234"));
        assert_eq!(rpm.patched_cves(), BTreeSet::from(["CVE-2024-1234"]));
    }
}
