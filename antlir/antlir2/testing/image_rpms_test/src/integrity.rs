/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use once_cell::sync::Lazy;
use regex::Regex;

static RPM_VERIFY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^((?:missing)|(?:.{9}))(?:\s+(c|d|g|l|r))?\s+(.*)$"#)
        .expect("definitely compiles")
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum RpmTest {
    /// file size differs
    Size,
    /// file mode (permissions and/or file type) differs
    Mode,
    /// checksum of file differs
    Digest,
    /// major/minor device number differs
    Device,
    /// symlink target differs
    Link,
    /// owner Uid differs
    User,
    /// owner Gid differs
    Group,
    /// Mtime differs
    Time,
    /// Capabilities differ
    Capabilities,
    /// file is entirely missing
    Missing,
    /// rpm --verify test passed
    Pass,
}

impl TryFrom<char> for RpmTest {
    type Error = Error;
    fn try_from(c: char) -> Result<Self> {
        match c {
            'S' => Ok(Self::Size),
            'M' => Ok(Self::Mode),
            '5' => Ok(Self::Digest),
            'D' => Ok(Self::Device),
            'L' => Ok(Self::Link),
            'U' => Ok(Self::User),
            'G' => Ok(Self::Group),
            'T' => Ok(Self::Time),
            'P' => Ok(Self::Capabilities),
            '.' => Ok(Self::Pass),
            _ => Err(anyhow!("unrecognized test character: {}", c)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileIntegrity {
    path: String,
    failed_tests: BTreeSet<RpmTest>,
}

impl FromStr for FileIntegrity {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if let Some(m) = RPM_VERIFY_RE.captures(s) {
            let tests = m.get(1).expect("must exist").as_str();
            let path = m.get(3).expect("must exist").as_str();
            let failed_tests = match tests {
                "missing" => BTreeSet::from([RpmTest::Missing]),
                _ => tests
                    .chars()
                    .map(RpmTest::try_from)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|t| *t != RpmTest::Pass)
                    .collect(),
            };
            Ok(Self {
                path: path.to_owned(),
                failed_tests,
            })
        } else {
            Err(anyhow!("'{s}' did not match the regex"))
        }
    }
}

#[derive(Parser)]
pub(crate) struct Integrity {
    #[clap(long)]
    layer: PathBuf,
    #[clap(long = "ignored-file")]
    ignored_files: Vec<String>,
    #[clap(long = "ignored-rpm")]
    ignored_rpms: Vec<String>,
}

impl Integrity {
    pub fn run(self) -> Result<()> {
        let verify_res = Command::new("rpm")
            .arg("--root")
            .arg(&self.layer)
            .arg("--verify")
            .arg("--all")
            // config files are pretty much expected to change
            // TODO(vmagro): really we should probably check that they are
            // "noreplace" config files (rpm will create .rpmnew files on any
            // changes, instead of moving changes to .rpmsave). In practice
            // though, every config file we are going to care about are
            // "noreplace", so don't complicate things
            .arg("--noconfig")
            .output()
            .context("failed to execute rpm")?;
        let ignored_files: HashSet<_> = self.ignored_files.into_iter().collect();
        let ignored_rpms: BTreeSet<_> = self.ignored_rpms.into_iter().collect();
        let mut failed_files = BTreeSet::new();
        for line in std::str::from_utf8(&verify_res.stdout)
            .context("output not utf8")?
            .lines()
        {
            let item: FileIntegrity = line.parse().context("while parsing rpm line")?;
            if !item.failed_tests.is_empty() {
                failed_files.insert(item);
            }
        }

        // query rpm to get the package(s) that own any given path
        let mut file_ownership = HashMap::<String, BTreeSet<String>>::new();
        let res = Command::new("rpm")
            .arg("--root=/layer")
            .arg("--query")
            .arg("--all")
            .arg("--queryformat=%{NAME} [%{FILENAMES} ]\n")
            .output()
            .context("failed to execute rpm")?;
        ensure!(res.status.success(), "'rpm' failed");
        for line in std::str::from_utf8(&res.stdout)
            .context("output not utf8")?
            .lines()
        {
            let mut iter = line.split_whitespace();
            let name = iter.next().context("rpm name must exist")?;
            for file in iter {
                file_ownership
                    .entry(file.to_owned())
                    .or_default()
                    .insert(name.to_owned());
            }
        }

        let mut overall_failure = false;
        for failure in &failed_files {
            let owners = file_ownership
                .get(&failure.path)
                .with_context(|| format!("no owner for '{}'", failure.path))?;
            if ignored_files.contains(&failure.path) {
                tracing::trace!(path = failure.path, "ignoring ignored file");
                continue;
            }
            let ignored_owners_intersection: BTreeSet<_> =
                owners.intersection(&ignored_rpms).collect();
            if !ignored_owners_intersection.is_empty() {
                tracing::trace!(
                    path = failure.path,
                    owners = format!("{ignored_owners_intersection:?}"),
                    "file is owned by ignored package(s)"
                );
                continue;
            }
            eprintln!(
                "{} owned by {owners:?}: {:?}",
                failure.path, failure.failed_tests
            );
            overall_failure = true;
        }
        ensure!(
            !overall_failure,
            "one or more files failed integrity checks"
        );

        Ok(())
    }
}
