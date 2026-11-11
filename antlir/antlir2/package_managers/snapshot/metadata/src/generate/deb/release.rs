/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use serde::Deserialize;
use snapshot_common::Checksums;

#[derive(Parser, Debug)]
pub(crate) struct GenerateRelease {
    /// Path to release.json (output of `snapshot parse deb release`)
    #[clap(long)]
    release_json: JsonFile<ReleaseJson>,
    /// Output path for the generated Release file
    out: PathBuf,
    #[clap(long)]
    components_json: JsonFile<Vec<ComponentArg>>,
}

/// Just the header fields we need from release.json.
#[derive(Debug, Clone, Deserialize)]
struct ReleaseJson {
    origin: Option<String>,
    label: Option<String>,
    suite: Option<String>,
    codename: Option<String>,
    date: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ComponentArg {
    arch: String,
    name: String,
    packages_txt: PathBuf,
}

struct ComponentEntry {
    arch: String,
    name: String,
    size: u64,
    checksums: Checksums,
}

impl GenerateRelease {
    pub(crate) fn run(self) -> Result<()> {
        let mut entries = Vec::new();
        for comp in self.components_json.into_inner() {
            let file = File::open(&comp.packages_txt)
                .with_context(|| format!("while opening '{}'", comp.packages_txt.display()))?;
            let size = file.metadata()?.len();
            entries.push(ComponentEntry {
                arch: comp.arch,
                name: comp.name,
                checksums: Checksums::from_reader(file).with_context(|| {
                    format!("while checksumming {}", comp.packages_txt.display())
                })?,
                size,
            });
        }

        // The Release file is intentionally unsigned: the only signing
        // key ever used here was a dummy whose secret is checked into
        // the repo, so a signature would prove nothing. Consumers read
        // the archive as a trusted source, and integrity is enforced
        // by the checksums Buck pins for every downloaded file.
        let release_content = build_release_content(self.release_json.as_inner(), &entries)?;

        let mut outfile = std::io::BufWriter::new(
            stdio_path::create(&self.out)
                .with_context(|| format!("creating {}", self.out.display()))?,
        );
        outfile.write_all(release_content.as_bytes())?;

        Ok(())
    }
}

fn build_release_content(release: &ReleaseJson, entries: &[ComponentEntry]) -> Result<String> {
    let mut out = String::new();

    if let Some(origin) = &release.origin {
        writeln!(out, "Origin: {origin}")?;
    }
    if let Some(label) = &release.label {
        writeln!(out, "Label: {label}")?;
    }
    if let Some(suite) = &release.suite {
        writeln!(out, "Suite: {suite}")?;
    }
    if let Some(codename) = &release.codename {
        writeln!(out, "Codename: {codename}")?;
    }
    writeln!(out, "Date: {}", release.date)?;
    if let Some(description) = &release.description {
        writeln!(out, "Description: {description}")?;
    }

    let archs: BTreeSet<&str> = entries.iter().map(|e| e.arch.as_str()).collect();
    writeln!(
        out,
        "Architectures: {}",
        archs.into_iter().collect::<Vec<_>>().join(" ")
    )?;

    let component_names: BTreeSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    writeln!(
        out,
        "Components: {}",
        component_names.into_iter().collect::<Vec<_>>().join(" ")
    )?;

    writeln!(out, "SHA256:")?;
    for entry in entries {
        writeln!(
            out,
            " {} {:>8} {}/binary-{}/Packages",
            entry
                .checksums
                .sha256_hex()
                .with_context(|| format!("missing sha256 for {}", entry.name))?,
            entry.size,
            entry.name,
            entry.arch,
        )?;
    }
    writeln!(out, "SHA1:")?;
    for entry in entries {
        writeln!(
            out,
            " {} {:>8} {}/binary-{}/Packages",
            entry
                .checksums
                .sha1_hex()
                .with_context(|| format!("missing sha1 for {}", entry.name))?,
            entry.size,
            entry.name,
            entry.arch,
        )?;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_content_is_unsigned_and_deterministic() {
        let release = ReleaseJson {
            origin: Some("Test".to_owned()),
            label: None,
            suite: Some("testsuite".to_owned()),
            codename: None,
            date: "Thu, 01 Jan 1970 00:00:00 UTC".to_owned(),
            description: None,
        };
        let entries = vec![ComponentEntry {
            arch: "amd64".to_owned(),
            name: "main".to_owned(),
            size: 42,
            checksums: Checksums::builder()
                .sha1("cd".repeat(20))
                .sha256("ab".repeat(32))
                .build()
                .expect("test checksums"),
        }];

        let first = build_release_content(&release, &entries).expect("build release content");
        let second = build_release_content(&release, &entries).expect("rebuild release content");
        assert_eq!(first, second, "release content must be deterministic");
        assert!(
            !first.contains("PGP"),
            "release content must not be signed: {first}"
        );
        assert!(first.starts_with("Origin: Test\n"));
        assert!(first.contains("main/binary-amd64/Packages"));
    }
}
