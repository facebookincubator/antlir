/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use size::Size;

#[derive(Parser)]
enum Args {
    RpmNames {
        path: PathBuf,
        #[clap(long)]
        /// Print details about installed rpms, don't run test
        print: bool,
    },
    RpmVerify,
}

#[derive(Debug)]
struct InstalledInfo {
    size: Size,
    required_by: BTreeSet<String>,
}

impl Display for InstalledInfo {
    #[deny(unused_variables)]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let InstalledInfo { size, required_by } = self;
        write!(f, "{size}")?;
        if !required_by.is_empty() {
            write!(f, " required by {}", required_by.iter().join(" "))?;
        }
        Ok(())
    }
}

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

fn main() -> Result<()> {
    let args = Args::parse();
    match args {
        Args::RpmNames { path, print } => {
            let res = Command::new("rpm")
                .arg("--root=/layer")
                .arg("-qa")
                .arg("--queryformat")
                .arg("%{RPMTAG_NAME} %{RPMTAG_SIZE}\\n")
                .output()
                .context("failed to execute rpm")?;
            ensure!(res.status.success(), "'rpm' failed");
            let mut installed = BTreeMap::<String, InstalledInfo>::new();
            for line in std::str::from_utf8(&res.stdout)
                .context("invalid utf8")?
                .lines()
            {
                let (name, size) = line
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
                    .arg("%{RPMTAG_NAME}\\n")
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
                        size: Size::from_bytes(size),
                        required_by,
                    },
                );
            }

            if print {
                let mut name_col_width = installed.keys().map(String::len).max().unwrap_or(0);
                if name_col_width < 20 {
                    name_col_width = 20;
                }
                for (name, info) in installed {
                    println!("{name:name_col_width$} {info}");
                }
                return Ok(());
            }
            let expected_names: BTreeSet<String> = std::fs::read_to_string(path)?
                .lines()
                .map(|l| {
                    l.split_whitespace()
                        .next()
                        .expect("always exists")
                        .to_string()
                })
                .collect();
            let installed_names: BTreeSet<String> = installed.keys().cloned().collect();
            similar_asserts::assert_eq!(
                expected: expected_names,
                installed: installed_names,
                "Installed rpms don't match. `buck run` this test with `-- --print` to generate a new source file"
            );
        }
        Args::RpmVerify => {
            let res = Command::new("rpm")
                .arg("--root=/layer")
                .arg("--verify")
                .arg("--all")
                .output()
                .context("failed to execute rpm")?;
            let mut failed = BTreeSet::new();
            for line in std::str::from_utf8(&res.stdout)
                .context("output not utf8")?
                .lines()
            {
                let item: FileIntegrity = line.parse().context("while parsing rpm line")?;
                if !item.failed_tests.is_empty() {
                    failed.insert(item);
                }
            }
            ensure!(
                failed.is_empty(),
                "one or more rpm-owned files failed verification: {failed:#?}"
            );
            // ensure that the rpm command did not fail in cases where we didn't
            // get any output
            ensure!(res.status.success(), "'rpm' failed");
        }
    }
    Ok(())
}
