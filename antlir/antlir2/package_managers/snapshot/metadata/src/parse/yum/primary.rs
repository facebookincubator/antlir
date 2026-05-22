/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use serde::de::Error as _;
use serde_with::DeserializeAs;
use serde_with::serde_as;
use snapshot_common::Checksums;
use snapshot_common::Package;

use super::Location;
use super::checksums_from_xml;

#[derive(Parser, Debug)]
pub(crate) struct ParsePrimary {
    /// Path to primary.xml
    #[clap(long)]
    primary_xml: PathBuf,
    #[clap(long)]
    basic_out: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    package: Vec<XmlPackage>,
}

#[serde_as]
#[derive(Debug, PartialEq, Deserialize)]
struct XmlPackage {
    name: String,
    arch: String,
    version: Version,
    #[serde_as(as = "PkgidChecksums")]
    #[serde(rename = "checksum")]
    pkgid: Checksums,
    location: Location,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Version {
    #[serde(alias = "@epoch")]
    epoch: u64,
    #[serde(alias = "@ver")]
    ver: String,
    #[serde(alias = "@rel")]
    rel: String,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}-{}", self.epoch, self.ver, self.rel)
    }
}

impl From<XmlPackage> for Package {
    fn from(p: XmlPackage) -> Self {
        Self {
            package: p.name,
            version: p.version.to_string(),
            architecture: p.arch,
            filename: p.location.href,
            checksums: p.pkgid,
        }
    }
}

fn parse<R: BufRead>(reader: R) -> Result<Vec<XmlPackage>> {
    let primary: Metadata = quick_xml::de::from_reader(reader).context("while reading xml")?;
    Ok(primary.package)
}

impl ParsePrimary {
    #[tracing::instrument(ret, err)]
    pub(crate) fn run(&self) -> Result<()> {
        let infile = BufReader::new(stdio_path::open(&self.primary_xml)?);

        let packages = parse(infile)
            .with_context(|| format!("while parsing {}", self.primary_xml.display()))?;

        let basic: Vec<Package> = packages.into_iter().map(Package::from).collect();

        let mut basic_out = BufWriter::new(stdio_path::create(&self.basic_out)?);
        serde_json::to_writer_pretty(&mut basic_out, &basic)?;
        basic_out.write_all(b"\n")?;

        Ok(())
    }
}

/// Deserialize a Checksum but make sure that it has `pkgid="YES"` on it.
struct PkgidChecksums;

impl<'de> DeserializeAs<'de, Checksums> for PkgidChecksums {
    fn deserialize_as<D>(deserializer: D) -> Result<Checksums, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Debug, Deserialize)]
        struct Xml {
            #[serde(rename = "@type")]
            ty: String,
            #[serde(rename = "$value")]
            text: String,
            #[serde(rename = "@pkgid")]
            pkgid: String,
        }
        let x = Xml::deserialize(deserializer).map_err(D::Error::custom)?;
        if x.pkgid != "YES" {
            return Err(D::Error::custom("checksum was not marked as pkgid"));
        }
        checksums_from_xml(&x.ty, x.text)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    const PRIMARY_XML_TXT: &str = include_str!("../../../../testdata/primary.xml");

    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(
            vec![
                XmlPackage {
                    name: "ModemManager".into(),
                    arch: "x86_64".into(),
                    version: Version {
                        epoch: 0,
                        ver: "1.18.0".into(),
                        rel: "1.el9".into(),
                    },
                    pkgid: Checksums::new_sha256(
                        "fdf8a5b8c7b0d6d52674489ce20991d963986efd404965b11a7213313b0cb4f8".into()
                    ),
                    location: Location {
                        href: "Packages/ModemManager-1.18.0-1.el9.x86_64.rpm".into(),
                    },
                },
                XmlPackage {
                    name: "ModemManager".into(),
                    arch: "x86_64".into(),
                    version: Version {
                        epoch: 0,
                        ver: "1.18.2".into(),
                        rel: "1.el9".into(),
                    },
                    pkgid: Checksums::new_sha256(
                        "4f376dbc093b4b34512a11457428e039b08d5c6cebc8bfc006a58fbbe30acfc4".into()
                    ),
                    location: Location {
                        href: "Packages/ModemManager-1.18.2-1.el9.x86_64.rpm".into(),
                    },
                },
                XmlPackage {
                    name: "NetworkManager".into(),
                    arch: "x86_64".into(),
                    version: Version {
                        epoch: 1,
                        ver: "1.51.2".into(),
                        rel: "1.el9".into(),
                    },
                    pkgid: Checksums::new_sha256(
                        "ef3aa03f3f6b345b11732d34fa2b8bdd2db985a8836b6e201ad6a03bce1236f7".into()
                    ),
                    location: Location {
                        href: "Packages/NetworkManager-1.51.2-1.el9.x86_64.rpm".into(),
                    },
                },
                XmlPackage {
                    name: "NetworkManager".into(),
                    arch: "x86_64".into(),
                    version: Version {
                        epoch: 1,
                        ver: "1.51.2".into(),
                        rel: "2.el9".into(),
                    },
                    pkgid: Checksums::new_sha256(
                        "80db4f57cd9112294c1b60b74e34d04092d74625baae43dcf1ad7a0cbac31c69".into()
                    ),
                    location: Location {
                        href: "Packages/NetworkManager-1.51.2-2.el9.x86_64.rpm".into(),
                    },
                }
            ],
            parse(Cursor::new(PRIMARY_XML_TXT)).expect("failed to parse test primary.xml")
        );
    }

    #[test]
    fn test_basic_package_serialization() {
        let packages =
            parse(Cursor::new(PRIMARY_XML_TXT)).expect("failed to parse test primary.xml");

        let basic: Vec<Package> = packages.into_iter().map(Package::from).collect();

        let json = serde_json::to_value(&basic).expect("failed to serialize");
        let first = &json[0];

        // The snapshot pipeline Entry struct requires top-level "filename" and
        // "checksums" fields. Verify they are present and correct.
        assert_eq!(
            first["filename"], "Packages/ModemManager-1.18.0-1.el9.x86_64.rpm",
            "filename should be the location href"
        );
        assert!(
            first["checksums"].is_object(),
            "checksums should be a top-level object"
        );
        assert!(
            first["checksums"]["sha256"].is_string(),
            "checksums should contain a sha256 field"
        );
        assert_eq!(first["package"], "ModemManager");
        assert_eq!(first["architecture"], "x86_64");

        // Verify that the old format fields are NOT present
        assert!(
            first.get("location").is_none(),
            "should not have nested location field"
        );
        assert!(first.get("pkgid").is_none(), "should not have pkgid field");
    }
}
