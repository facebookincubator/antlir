/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::UnitName;

#[derive(Debug, Error, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Problem {
    /// /var/run is automatically migrated to /run, but the unit file should be
    /// changed to match
    #[error("{key} refers to a path under /var/run ({path:?})")]
    LegacyVarRun { key: String, path: PathBuf },

    #[error("{path:?} is not executable: {reason}")]
    NotExecutable { path: PathBuf, reason: String },

    #[error("missing man page {page}")]
    MissingManPage { page: String },

    #[error("uncategorized error from systemd-analyze verify: {raw}")]
    Uncategorized { raw: String },
}

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

impl From<&str> for Problem {
    fn from(s: &str) -> Self {
        if let Some(cap) =
            regex!(r##"(.*)= references a path below legacy directory /var/run/, updating (/var/run/.*?)\s+.*"##)
                .captures(s)
        {
            return Self::LegacyVarRun {
                key: cap.get(1).unwrap().as_str().to_owned(),
                path: cap.get(2).unwrap().as_str().into(),
            };
        }
        if let Some(cap) = regex!(r##"^Command\s+(.*)\s+is not executable:\s+(.*)$"##).captures(s) {
            return Self::NotExecutable {
                path: cap.get(1).unwrap().as_str().into(),
                reason: cap.get(2).unwrap().as_str().to_owned(),
            };
        }
        if let Some(cap) = regex!(r##"^Command\s'man\s+(.*)' failed with code \d+$"##).captures(s) {
            return Self::MissingManPage {
                page: cap.get(1).unwrap().as_str().into(),
            };
        }
        Self::Uncategorized { raw: s.to_owned() }
    }
}

/// Call 'systemd-analyze verify' for some set of units and parse any problems
/// that were discovered. Note that the returned set may be a superset of the
/// units that were passed in, if systemd-analyze finds any issues in the
/// dependencies of the requested units. Note also that this function returns a
/// set of [Problem]s, however in practice this will usually just be the first
/// problem encountered (depending on the class of problem). Some work may be
/// attempted in the future to discover all problems instead of sometimes
/// bailing early.
pub fn verify<I, S>(units: I) -> anyhow::Result<BTreeMap<UnitName, BTreeSet<Problem>>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("systemd-analyze")
        .arg("verify")
        // explicitly turn off man page checking for now, we explicitly can
        // parse man page failures, but systemd only prints the first error it
        // finds, and we much prefer to find actual problems other than a man
        // page being unavailable
        .arg("--man=off")
        .args(units)
        .output()
        .context("failed to run 'systemd-analyze verify'")?;

    let stderr =
        std::str::from_utf8(&output.stderr).context("invalid utf-8 in systemd-analyze output")?;
    let mut problems = BTreeMap::new();
    for line in stderr.lines() {
        if let Some(cap) =
            regex!(r##"^(?P<unit>.*?)(?::(?P<line>\d+))?:(?P<prob>.*)$"##).captures(line)
        {
            let unit: PathBuf = cap.name("unit").unwrap().as_str().into();
            let unit = unit
                .file_name()
                .expect("units always have a filename")
                .to_str()
                // already parsed whole output as utf-8 so this has to succeed
                .expect("unit filename always utf-8")
                .into();
            let problem = cap.name("prob").unwrap().as_str().trim_start().into();
            problems
                .entry(unit)
                .or_insert_with(BTreeSet::new)
                .insert(problem);
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn problem_parse() {
        assert_eq!(
            Problem::LegacyVarRun{key: "PIDFile".into(), path: "/var/run/abc.pid".into()},
            "PIDFile= references a path below legacy directory /var/run/, updating /var/run/abc.pid → /run/abc.pid; please update the unit file accordingly.".into()
        );
        assert_eq!(
            Problem::NotExecutable {
                path: "/usr/sbin/quotaon".into(),
                reason: "No such file or directory".into()
            },
            "Command /usr/sbin/quotaon is not executable: No such file or directory".into()
        );
        assert_eq!(
            Problem::MissingManPage {
                page: "systemd.special(7)".into()
            },
            "Command 'man systemd.special(7)' failed with code 16".into()
        );
        assert_eq!(
            Problem::Uncategorized {
                raw: "some random string that is not understood".into()
            },
            "some random string that is not understood".into()
        );
    }
}
