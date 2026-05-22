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
use snapshot_common::Checksums;
use snapshot_common::Package;

#[derive(Parser, Debug)]
pub(crate) struct ParsePackages {
    /// Path to Packages index file (uncompressed)
    packages: PathBuf,
    /// Output path (or - for stdout)
    out: PathBuf,
}

/// Intermediate builder accumulating lines for a single stanza.
#[derive(Debug, Default)]
struct PackageBuilder {
    package: Option<String>,
    version: Option<String>,
    architecture: Option<String>,
    filename: Option<String>,
    sha256: Option<String>,
}

impl PackageBuilder {
    fn build(self) -> Result<Package> {
        Ok(Package {
            package: self.package.context("missing required Package field")?,
            version: self.version.context("missing required Version field")?,
            architecture: self
                .architecture
                .context("missing required Architecture field")?,
            filename: self.filename.context("missing required Filename field")?,
            checksums: Checksums::builder()
                .sha256(self.sha256.context("missing required SHA256 field")?)
                .build()?,
        })
    }

    fn is_empty(&self) -> bool {
        self.package.is_none()
    }
}

fn parse<R: BufRead>(reader: R) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    let mut builder = PackageBuilder::default();

    for line in reader.lines() {
        let line = line.context("reading line")?;

        // Empty line separates stanzas
        if line.is_empty() {
            if !builder.is_empty() {
                packages.push(
                    builder
                        .build()
                        .with_context(|| format!("package #{}", packages.len() + 1))?,
                );
                builder = PackageBuilder::default();
            }
            continue;
        }

        // Continuation line (starts with space or tab) — part of a multi-line field value. We
        // don't care about any of those
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }

        // Key-value field — only extract the fields we care about
        if let Some((key, value)) = line.split_once(": ") {
            match key {
                "Package" => builder.package = Some(value.to_owned()),
                "Version" => builder.version = Some(value.to_owned()),
                "Architecture" => builder.architecture = Some(value.to_owned()),
                "Filename" => builder.filename = Some(value.to_owned()),
                "SHA256" => builder.sha256 = Some(value.to_owned()),
                _ => {}
            }
        }
    }

    // Handle last stanza if file doesn't end with a blank line
    if !builder.is_empty() {
        packages.push(
            builder
                .build()
                .with_context(|| format!("package #{}", packages.len() + 1))?,
        );
    }

    Ok(packages)
}

impl ParsePackages {
    #[tracing::instrument(ret, err)]
    pub(crate) fn run(&self) -> Result<()> {
        let infile = BufReader::new(stdio_path::open(&self.packages)?);
        let out =
            parse(infile).with_context(|| format!("while parsing {}", self.packages.display()))?;

        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer_pretty(&mut outfile, &out)?;
        outfile.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_parse() {
        let input = include_str!("../../../../testdata/deb/Packages");
        let packages = parse(Cursor::new(input)).expect("failed to parse test Packages");

        assert_eq!(packages.len(), 4);

        // 0ad
        let p = &packages[0];
        assert_eq!(p.package, "0ad");
        assert_eq!(p.version, "0.27.0-2+b1");
        assert_eq!(p.architecture, "amd64");
        assert_eq!(p.filename, "pool/main/0/0ad/0ad_0.27.0-2+b1_amd64.deb");
        assert_eq!(
            p.checksums,
            Checksums::new_sha256(
                "704dae7df79a35ed1bd3a0a8d5d1b9ce7a4ecdef74ddff72114d9523d63710eb".into()
            )
        );

        // 0ad-data
        let p = &packages[1];
        assert_eq!(p.package, "0ad-data");
        assert_eq!(p.version, "0.27.0-1");
        assert_eq!(p.architecture, "all");
        assert_eq!(p.filename, "pool/main/0/0ad-data/0ad-data_0.27.0-1_all.deb");
        assert_eq!(
            p.checksums,
            Checksums::new_sha256(
                "7a25d66175bdc75cbd6a90e98b71dcb0d76f7763c589bd71880503f1c6185e4c".into()
            )
        );

        // 0ad-data-common
        let p = &packages[2];
        assert_eq!(p.package, "0ad-data-common");
        assert_eq!(p.version, "0.27.0-1");
        assert_eq!(p.architecture, "all");
        assert_eq!(
            p.filename,
            "pool/main/0/0ad-data/0ad-data-common_0.27.0-1_all.deb"
        );

        // lib0install-solver-ocaml
        let p = &packages[3];
        assert_eq!(p.package, "lib0install-solver-ocaml");
        assert_eq!(p.version, "2.18-1+b4");
        assert_eq!(p.architecture, "amd64");
        assert_eq!(
            p.filename,
            "pool/main/0/0install-solver/lib0install-solver-ocaml_2.18-1+b4_amd64.deb"
        );
        assert_eq!(
            p.checksums,
            Checksums::new_sha256(
                "1047cc7df5b50a3b3b2db16c6270da90ad91cdb2ba7e011c6938467ea1fb3f94".into()
            )
        );
    }

    #[test]
    fn test_parse_empty() {
        let packages = parse(Cursor::new("")).expect("failed to parse empty input");
        assert!(packages.is_empty());
    }

    #[test]
    fn test_parse_missing_required_field() {
        let input = "Package: broken\nVersion: 1.0\n\n";
        let err = parse(Cursor::new(input)).expect_err("should fail for missing fields");
        assert!(
            format!("{err:#}").contains("missing required"),
            "unexpected error: {err:#}"
        );
    }
}
