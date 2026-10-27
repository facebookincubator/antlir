/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use snapshot_common::Checksums;

#[derive(Parser, Debug)]
pub(crate) struct ParseRelease {
    /// Path to Release or InRelease
    release: PathBuf,
    /// Output path (or - for stdout)
    out: PathBuf,
}

/// Intermediate builder with all fields optional, used during parsing.
#[derive(Debug, Default)]
struct OutBuilder {
    origin: Option<String>,
    label: Option<String>,
    suite: Option<String>,
    version: Option<String>,
    codename: Option<String>,
    date: Option<String>,
    valid_until: Option<String>,
    architectures: Option<Vec<String>>,
    description: Option<String>,
    acquire_by_hash: bool,
    changelogs: Option<String>,
    snapshots: Option<String>,
    not_automatic: bool,
    but_automatic_upgrades: bool,
    components: BTreeMap<String, Component>,
}

impl OutBuilder {
    fn build(self) -> Result<Out> {
        anyhow::ensure!(
            self.suite.is_some() || self.codename.is_some(),
            "missing both Suite and Codename fields (at least one is required)"
        );
        Ok(Out {
            origin: self.origin,
            label: self.label,
            suite: self.suite,
            version: self.version,
            codename: self.codename,
            date: self.date.context("missing required Date field")?,
            valid_until: self.valid_until,
            architectures: self
                .architectures
                .context("missing required Architectures field")?,
            description: self.description,
            acquire_by_hash: self.acquire_by_hash,
            changelogs: self.changelogs,
            snapshots: self.snapshots,
            not_automatic: self.not_automatic,
            but_automatic_upgrades: self.but_automatic_upgrades,
            components: self.components,
        })
    }
}

/// Parsed representation of a Debian Release / InRelease file.
///
/// See <https://wiki.debian.org/DebianRepository/Format#A.22Release.22_files>
/// for the full specification.
#[derive(Debug, PartialEq, Serialize)]
struct Out {
    /// Optional. Single line of free form text indicating the origin of the
    /// repository (e.g. "Debian"). Used by package management tools for display
    /// and pinning.
    origin: Option<String>,
    /// Optional. Single line of free form text, typically used in repositories
    /// split over multiple media such as CDs.
    label: Option<String>,
    /// The suite name (e.g. "stable", "testing", "unstable"). At least one of
    /// `suite` or `codename` is required.
    suite: Option<String>,
    /// Optional. Version of the release, usually a sequence of integers
    /// separated by '.' (e.g. "13.3").
    version: Option<String>,
    /// The codename of the release (e.g. "trixie", "bookworm"). At least one of
    /// `suite` or `codename` is required.
    codename: Option<String>,
    /// Required. The time at which the Release file was created. Format is
    /// RFC 2822 date in UTC (e.g. "Sat, 10 Jan 2026 10:28:38 UTC").
    date: String,
    /// Optional. The time at which the Release file should be considered expired.
    /// Same format as `date`.
    valid_until: Option<String>,
    /// Required. Whitespace-separated list of supported Debian machine
    /// architectures. The presence of "all" indicates that arch-independent
    /// packages have their own separate index files.
    architectures: Vec<String>,
    /// Optional. Free form text describing the release
    /// (e.g. "Debian 13.3 Released 10 January 2026").
    description: Option<String>,
    /// Whether the server supports "by-hash" locations as an alternative to
    /// canonical index file paths. Default false.
    acquire_by_hash: bool,
    /// Optional. URL template for fetching package changelogs, with
    /// `@CHANGEPATH@` as a placeholder. The special value "no" means changelogs
    /// are not provided.
    changelogs: Option<String>,
    /// Optional. URL template for fetching archive snapshots, with
    /// `@SNAPSHOTID@` as a placeholder (timestamp in `%Y%m%dT%H%M%SZ` format).
    /// The special value "no" means snapshots are not provided.
    snapshots: Option<String>,
    /// If true, packages from this repository should not be installed or
    /// upgraded without explicit user consent (APT assigns priority 1).
    /// Default false.
    not_automatic: bool,
    /// Only valid when `not_automatic` is true. If true, package upgrades from
    /// this repository are automatic when the installed version is higher than
    /// versions in other sources (APT assigns priority 100). Default false.
    but_automatic_upgrades: bool,
    /// Map of component name (e.g. "main", "contrib") to its Packages index
    /// files, keyed by architecture.
    components: BTreeMap<String, Component>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Component {
    /// Packages index file to download for each architecture
    packages: BTreeMap<String, BlobDl>,
}

#[derive(Debug, PartialEq, Serialize)]
struct BlobDl {
    href: String,
    size: u64,
    checksums: Checksums,
    filetype: String,
}

fn compression_rank(filetype: &str) -> u8 {
    match filetype {
        "xz" => 2,
        "gz" => 1,
        _ => 0,
    }
}

/// Which checksum section an entry line was parsed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionHash {
    Sha256,
    Sha512,
}

/// One checksum-section entry: hashes recorded per file path, merged across
/// sections (a Release file may carry SHA256, SHA512, or both).
#[derive(Debug, Default)]
struct FileHashes {
    size: u64,
    sha256: Option<String>,
    sha512: Option<String>,
}

fn parse<R: BufRead>(reader: R) -> Result<Out> {
    let mut out = OutBuilder::default();
    let mut section: Option<SectionHash> = None;
    let mut in_pgp_preamble = false;
    // path -> hashes, merged across checksum sections
    let mut files: BTreeMap<String, FileHashes> = BTreeMap::new();

    for line in reader.lines() {
        let line = line.context("reading line")?;

        // Handle InRelease PGP signed message header
        if line == "-----BEGIN PGP SIGNED MESSAGE-----" {
            in_pgp_preamble = true;
            continue;
        }
        if in_pgp_preamble {
            if line.is_empty() {
                in_pgp_preamble = false;
            }
            continue;
        }

        // PGP signature block terminates the content
        if line == "-----BEGIN PGP SIGNATURE-----" {
            break;
        }

        // Checksum sections (SHA256 and/or SHA512)
        if line == "SHA256:" {
            section = Some(SectionHash::Sha256);
            continue;
        }
        if line == "SHA512:" {
            section = Some(SectionHash::Sha512);
            continue;
        }

        // A checksum section is a long block that is indented. We remain inside
        // that block and record checksums until we see a non-indented line that
        // marks the end
        if let Some(section_hash) = section {
            if let Some(rest) = line.strip_prefix(' ') {
                let mut parts = rest.split_whitespace();
                let hash = parts.next().context("missing hash")?.to_owned();
                let size: u64 = parts
                    .next()
                    .context("missing size")?
                    .parse()
                    .context("invalid size")?;
                let path = parts.next().context("missing path")?.to_owned();

                let entry = files.entry(path).or_default();
                entry.size = size;
                match section_hash {
                    SectionHash::Sha256 => entry.sha256 = Some(hash),
                    SectionHash::Sha512 => entry.sha512 = Some(hash),
                }
            } else {
                // Non-indented line means we've left the checksum sections
                break;
            }
            continue;
        }

        // Header key-value fields (before any checksum section)
        if let Some((key, value)) = line.split_once(": ") {
            match key {
                "Origin" => out.origin = Some(value.to_owned()),
                "Label" => out.label = Some(value.to_owned()),
                "Suite" => out.suite = Some(value.to_owned()),
                "Version" => out.version = Some(value.to_owned()),
                "Codename" => out.codename = Some(value.to_owned()),
                "Date" => out.date = Some(value.to_owned()),
                "Valid-Until" => out.valid_until = Some(value.to_owned()),
                "Architectures" => {
                    out.architectures = Some(value.split_whitespace().map(String::from).collect());
                }
                "Description" => out.description = Some(value.to_owned()),
                "Acquire-By-Hash" => out.acquire_by_hash = value == "yes",
                "Changelogs" => out.changelogs = Some(value.to_owned()),
                "Snapshots" => out.snapshots = Some(value.to_owned()),
                "NotAutomatic" => out.not_automatic = value == "yes",
                "ButAutomaticUpgrades" => out.but_automatic_upgrades = value == "yes",
                _ => {}
            }
        }
    }

    // Fold merged per-path hashes into components, keeping the best
    // compression per (component, arch) like before
    for (path, hashes) in &files {
        let segments: Vec<&str> = path.splitn(4, '/').collect();
        // Match <component>/binary-<arch>/Packages[.<ext>]
        if segments.len() < 3 {
            continue;
        }
        let component = segments[0];
        let arch = match segments[1].strip_prefix("binary-") {
            Some(a) => a,
            None => continue,
        };
        let filename = segments[2];
        let (is_packages, filetype) = match filename {
            "Packages" => (true, ""),
            "Packages.gz" => (true, "gz"),
            "Packages.xz" => (true, "xz"),
            _ => (false, ""),
        };
        // Skip debian-installer paths (segments.len() > 3 means
        // there's extra nesting like debian-installer/binary-<arch>/...)
        if !is_packages || segments.len() > 3 {
            continue;
        }

        let checksums = Checksums::builder()
            .maybe_sha256(hashes.sha256.clone())
            .maybe_sha512(hashes.sha512.clone())
            .build()?;
        let entry = BlobDl {
            href: path.to_owned(),
            size: hashes.size,
            checksums,
            filetype: filetype.to_owned(),
        };

        let comp = out
            .components
            .entry(component.to_owned())
            .or_insert_with(|| Component {
                packages: BTreeMap::new(),
            });

        let dominated = comp.packages.get(arch).is_some_and(|existing| {
            compression_rank(&existing.filetype) >= compression_rank(filetype)
        });
        if !dominated {
            comp.packages.insert(arch.to_owned(), entry);
        }
    }

    out.build()
}

impl ParseRelease {
    #[tracing::instrument(ret, err)]
    pub(crate) fn run(&self) -> Result<()> {
        let infile = BufReader::new(stdio_path::open(&self.release)?);
        let out =
            parse(infile).with_context(|| format!("while parsing {}", self.release.display()))?;

        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer_pretty(&mut outfile, &out)?;
        outfile.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_parse() {
        let out = parse(Cursor::new(include_str!(
            "../../../../testdata/deb/trixie-InRelease"
        )))
        .expect("failed to parse test trixie-InRelease");

        // Mandatory header fields
        assert_eq!(out.origin.as_deref(), Some("Debian"));
        assert_eq!(out.label.as_deref(), Some("Debian"));
        assert_eq!(out.suite.as_deref(), Some("stable"));
        assert_eq!(out.version.as_deref(), Some("13.3"));
        assert_eq!(out.codename.as_deref(), Some("trixie"));
        assert_eq!(out.date, "Sat, 10 Jan 2026 10:28:38 UTC");
        assert_eq!(out.valid_until, None);
        assert_eq!(
            out.architectures,
            vec![
                "all", "amd64", "arm64", "armel", "armhf", "i386", "ppc64el", "riscv64", "s390x"
            ],
        );
        assert_eq!(
            out.description.as_deref(),
            Some("Debian 13.3 Released 10 January 2026")
        );
        assert!(out.acquire_by_hash);
        assert_eq!(
            out.changelogs.as_deref(),
            Some("https://metadata.ftp-master.debian.org/changelogs/@CHANGEPATH@_changelog")
        );
        assert_eq!(out.snapshots, None);
        assert!(!out.not_automatic);
        assert!(!out.but_automatic_upgrades);

        assert_eq!(
            out.components.keys().collect::<Vec<_>>(),
            vec!["contrib", "main", "non-free", "non-free-firmware"],
        );

        // Each component should have one entry per architecture, preferring xz
        let contrib = &out.components["contrib"];
        assert_eq!(
            contrib.packages.keys().collect::<Vec<_>>(),
            vec![
                "all", "amd64", "arm64", "armel", "armhf", "i386", "ppc64el", "riscv64", "s390x"
            ],
        );
        assert_eq!(
            contrib.packages["amd64"],
            BlobDl {
                href: "contrib/binary-amd64/Packages.xz".into(),
                size: 53848,
                checksums: Checksums::new_sha256(hex!(
                    "9979608b93622d99f0b8bd1143c8ba7039a5d23bd3fcebb8331a1b4b531c96d2"
                )),
                filetype: "xz".into(),
            },
        );

        let main = &out.components["main"];
        assert_eq!(
            main.packages["amd64"],
            BlobDl {
                href: "main/binary-amd64/Packages.xz".into(),
                size: 9670308,
                checksums: Checksums::new_sha256(hex!(
                    "d103f0fd4603869384a7486be3682dd00de0ac665b86a44c3b550ad21306d23d"
                )),
                filetype: "xz".into(),
            },
        );

        let non_free = &out.components["non-free"];
        assert_eq!(
            non_free.packages["amd64"],
            BlobDl {
                href: "non-free/binary-amd64/Packages.xz".into(),
                size: 100252,
                checksums: Checksums::new_sha256(hex!(
                    "873a1152a9641bbbd32ae06d9bea40c0eb9486caeea340a786d4ab84cb6de6c5"
                )),
                filetype: "xz".into(),
            },
        );

        let nff = &out.components["non-free-firmware"];
        assert_eq!(
            nff.packages["amd64"],
            BlobDl {
                href: "non-free-firmware/binary-amd64/Packages.xz".into(),
                size: 6884,
                checksums: Checksums::new_sha256(hex!(
                    "a05bfaf35b64eb8271e374f617b3b75b5ba24dacfa86fd7ddbec5a60d99a17da"
                )),
                filetype: "xz".into(),
            },
        );
    }

    #[test]
    fn test_parse_sha512_only() {
        // Ubuntu 26.10 (stonking) ships an InRelease with only a SHA512
        // section; the entries must still land in components.
        let input = concat!(
            "Origin: Ubuntu\n",
            "Suite: stonking\n",
            "Codename: stonking\n",
            "Date: Thu, 15 Oct 2026 12:18:34 UTC\n",
            "Architectures: amd64 arm64\n",
            "Description: Ubuntu Stonking 26.10\n",
            "SHA512:\n",
            " a820c19528163ad4db0b1873d56219204a1249d02413b6df0dfdb40a1ece9fca008f40917072f216785d0fa88ca06363db217ed5e80ef6a541f487c309fff7ed 6508829 main/binary-amd64/Packages\n",
            " 5922e68d2f32b224335d340f020294c04c2e16fbd2b514e3fc071f250208dc9ab8460a0e97528613e1d89d7993dae955176bde0c010ff39f0b2db9f52f70c492 6508829 main/binary-amd64/Packages\n",
            " 2405a8574932616f05e3ca9585ac180d858525a69f81095b8b86958594162bc8de2939400b986fa058ff1ab84941dcf3a56c6f6700070f1a10a9673989a294b2 6520091 main/binary-arm64/Packages\n",
        );
        let out = parse(Cursor::new(input)).expect("failed to parse sha512-only input");

        assert_eq!(out.components.keys().collect::<Vec<_>>(), vec!["main"]);
        let main = &out.components["main"];
        assert_eq!(
            main.packages.keys().collect::<Vec<_>>(),
            vec!["amd64", "arm64"],
        );
        assert_eq!(
            main.packages["amd64"],
            BlobDl {
                href: "main/binary-amd64/Packages".into(),
                size: 6508829,
                checksums: Checksums::new_sha512(hex!(
                    "5922e68d2f32b224335d340f020294c04c2e16fbd2b514e3fc071f250208dc9ab8460a0e97528613e1d89d7993dae955176bde0c010ff39f0b2db9f52f70c492"
                )),
                filetype: "".into(),
            },
        );
        assert_eq!(
            main.packages["arm64"],
            BlobDl {
                href: "main/binary-arm64/Packages".into(),
                size: 6520091,
                checksums: Checksums::new_sha512(hex!(
                    "2405a8574932616f05e3ca9585ac180d858525a69f81095b8b86958594162bc8de2939400b986fa058ff1ab84941dcf3a56c6f6700070f1a10a9673989a294b2"
                )),
                filetype: "".into(),
            },
        );
    }

    #[test]
    fn test_parse_sha256_and_sha512_merge() {
        // When both sections are present, hashes merge per path.
        let input = concat!(
            "Suite: stonking\n",
            "Date: Thu, 15 Oct 2026 12:18:34 UTC\n",
            "Architectures: amd64\n",
            "SHA256:\n",
            " abababababababababababababababababababababababababababababababab 1271815 main/binary-amd64/Packages.gz\n",
            "SHA512:\n",
            " a820c19528163ad4db0b1873d56219204a1249d02413b6df0dfdb40a1ece9fca008f40917072f216785d0fa88ca06363db217ed5e80ef6a541f487c309fff7ed 1271815 main/binary-amd64/Packages.gz\n",
        );
        let out = parse(Cursor::new(input)).expect("failed to parse mixed input");

        let main = &out.components["main"];
        let entry = &main.packages["amd64"];
        assert_eq!(entry.href, "main/binary-amd64/Packages.gz");
        assert_eq!(entry.filetype, "gz");
        assert_eq!(
            entry.checksums.sha256_hex().as_deref(),
            Some("abababababababababababababababababababababababababababababababab"),
        );
        assert_eq!(
            entry.checksums.sha512_hex().as_deref(),
            Some(
                "a820c19528163ad4db0b1873d56219204a1249d02413b6df0dfdb40a1ece9fca008f40917072f216785d0fa88ca06363db217ed5e80ef6a541f487c309fff7ed"
            ),
        );
    }
}
