/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::PathBuf;

use antlir2_facts::RoDatabase;
use antlir2_facts::fact::rpm::Rpm;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use clap::Parser;

#[derive(Parser)]
pub(crate) struct Names {
    path: PathBuf,
    #[clap(long)]
    facts_db: PathBuf,
    #[clap(long)]
    not_installed: bool,
    #[clap(long)]
    /// Print details about installed rpms, don't run test
    print: bool,
    #[clap(long)]
    /// Compare source package names instead of binary package names. A single
    /// source package (eg 'systemd') often produces many binary packages (eg
    /// 'systemd', 'systemd-libs', 'systemd-udev'); collapsing to source names
    /// is less precise but more stable across minor packaging changes.
    source_names: bool,
}

/// Derive the source package name from an rpm's `SOURCERPM` header, which looks
/// like `systemd-252-45.el9.src.rpm`, keeping just the name (eg `systemd`).
/// Falls back to the binary package name for rpms with no source rpm (eg
/// `gpg-pubkey`, whose `SOURCERPM` is unset and renders as the string `(none)`,
/// which fails to parse).
fn source_package_name(rpm: &Rpm) -> String {
    rpm.source_rpm()
        .and_then(parse_source_name)
        .unwrap_or_else(|| rpm.name().to_owned())
}

/// Parse the source package name out of a `SOURCERPM` header value like
/// `systemd-252-45.el9.src.rpm`. An rpm filename is
/// `name-[epoch:]version-release.arch.rpm`, so we strip fields off the right in
/// order: `.rpm`, then arch, release, and the epoch:version chunk, leaving the
/// name. Returns `None` for values that don't have this shape (eg `(none)`).
fn parse_source_name(source_rpm: &str) -> Option<String> {
    let nevra = source_rpm.trim_end_matches(".rpm");
    let (nevr, _arch) = nevra.rsplit_once('.')?;
    let (nev, _release) = nevr.rsplit_once('-')?;
    let (name, _epoch_version) = nev.rsplit_once('-')?;
    Some(name.to_owned())
}

impl Names {
    pub fn run(self) -> Result<()> {
        let facts = RoDatabase::open(&self.facts_db).context("while opening facts db")?;
        let installed_names: BTreeSet<String> = facts
            .iter::<Rpm>()
            .context("while getting rpms")?
            .filter_map(|r| {
                if self.source_names {
                    // gpg-pubkey entries are not real packages (they have no
                    // source rpm); exclude them rather than listing them as
                    // themselves via the fallback.
                    if r.name().starts_with("gpg-pubkey") {
                        return None;
                    }
                    Some(source_package_name(&r))
                } else {
                    Some(r.name().to_owned())
                }
            })
            .collect();

        if self.print {
            for name in &installed_names {
                println!("{name}");
            }
            return Ok(());
        }

        let expected_names: BTreeSet<String> = std::fs::read_to_string(self.path)?
            .lines()
            .map(|l| {
                l.split_whitespace()
                    .next()
                    .expect("always exists")
                    .to_string()
            })
            .collect();
        if !self.not_installed {
            similar_asserts::assert_eq!(
                expected: expected_names,
                installed: installed_names,
                "Installed rpms don't match. `buck run` this test with `-- --print` to generate a new source file"
            );
        } else {
            let unexpected_names: Vec<String> = expected_names
                .into_iter()
                .filter(|i| installed_names.contains(i))
                .collect();
            ensure!(
                unexpected_names.is_empty(),
                "Unexpected rpms installed in image: {}",
                unexpected_names.join(", ")
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use antlir2_facts::fact::rpm::Rpm;

    use super::source_package_name;

    fn rpm(name: &str, source_rpm: Option<&str>) -> Rpm {
        Rpm::builder()
            .name(name)
            .version("1")
            .release("2.el9")
            .arch("x86_64")
            .maybe_source_rpm(source_rpm)
            .build()
    }

    #[test]
    fn strips_version_release_and_suffix() {
        assert_eq!(
            source_package_name(&rpm("systemd-libs", Some("systemd-252-45.el9.src.rpm"))),
            "systemd"
        );
    }

    #[test]
    fn keeps_hyphenated_source_name() {
        assert_eq!(
            source_package_name(&rpm("python3-libs", Some("python3-3.9.21-1.el9.src.rpm"))),
            "python3"
        );
    }

    #[test]
    fn source_name_unrelated_to_binary_name() {
        // The source name can be completely unrelated to the binary name; only
        // SOURCERPM is authoritative.
        assert_eq!(
            source_package_name(&rpm("bind-utils", Some("bind-9.16.23-18.el9.src.rpm"))),
            "bind"
        );
    }

    #[test]
    fn strips_epoch_from_version() {
        // The epoch is part of the epoch:version chunk that gets stripped.
        assert_eq!(
            source_package_name(&rpm("grub2-tools", Some("grub2-1:2.06-95.el9.src.rpm"))),
            "grub2"
        );
    }

    #[test]
    fn falls_back_to_binary_name_when_no_source_rpm() {
        assert_eq!(source_package_name(&rpm("gpg-pubkey", None)), "gpg-pubkey");
    }

    #[test]
    fn falls_back_to_binary_name_when_source_rpm_is_none_string() {
        // rpm renders an absent SOURCERPM as the literal string "(none)".
        assert_eq!(
            source_package_name(&rpm("gpg-pubkey", Some("(none)"))),
            "gpg-pubkey"
        );
    }
}
