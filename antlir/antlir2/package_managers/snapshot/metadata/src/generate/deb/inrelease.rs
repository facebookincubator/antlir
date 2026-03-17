/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use sequoia_openpgp::Cert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::stream::Message;
use sequoia_openpgp::serialize::stream::Signer;
use serde::Deserialize;
use snapshot_common::Checksums;

const POLICY: &StandardPolicy = &StandardPolicy::new();

#[derive(Parser, Debug)]
pub(crate) struct GenerateInRelease {
    /// Path to release.json (output of `snapshot parse deb release`)
    #[clap(long)]
    release_json: JsonFile<ReleaseJson>,
    /// Architecture to include (e.g. "amd64", "arm64")
    #[clap(long)]
    arch: String,
    /// Path to PGP secret key for signing
    #[clap(long)]
    signing_key: PathBuf,
    /// Output path for the generated InRelease file
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
    name: String,
    packages_txt: PathBuf,
}

struct ComponentEntry {
    name: String,
    size: u64,
    checksums: Checksums,
}

impl GenerateInRelease {
    pub(crate) fn run(self) -> Result<()> {
        let mut entries = Vec::new();
        for comp in self.components_json.into_inner() {
            let file = File::open(&comp.packages_txt)
                .with_context(|| format!("while opening '{}'", comp.packages_txt.display()))?;
            let size = file.metadata()?.len();
            entries.push(ComponentEntry {
                name: comp.name,
                checksums: Checksums::from_reader(file).with_context(|| {
                    format!("while checksumming {}", comp.packages_txt.display())
                })?,
                size,
            });
        }

        let release_content =
            build_release_content(self.release_json.as_inner(), &self.arch, &entries)?;

        let key_data = std::fs::read(&self.signing_key)
            .with_context(|| format!("reading signing key {}", self.signing_key.display()))?;
        let cert = Cert::from_bytes(&key_data).context("parsing PGP signing key")?;

        let signing_key = cert
            .keys()
            .with_policy(POLICY, None)
            .supported()
            .alive()
            .revoked(false)
            .for_signing()
            .next()
            .context("no valid signing key found in cert")?
            .key()
            .clone()
            .parts_into_secret()
            .map_err(|_| anyhow::anyhow!("signing key has no secret material"))?
            .into_keypair()
            .context("creating keypair from secret key")?;

        let mut output = Vec::new();
        {
            let message = Message::new(&mut output);
            let mut signer = Signer::new(message, signing_key)?.cleartext().build()?;
            signer.write_all(release_content.as_bytes())?;
            signer.finalize()?;
        }

        let mut outfile = std::io::BufWriter::new(
            stdio_path::create(&self.out)
                .with_context(|| format!("creating {}", self.out.display()))?,
        );
        outfile.write_all(&output)?;

        Ok(())
    }
}

fn build_release_content(
    release: &ReleaseJson,
    arch: &str,
    entries: &[ComponentEntry],
) -> Result<String> {
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
    writeln!(out, "Architectures: {arch}")?;

    let component_names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    writeln!(out, "Components: {}", component_names.join(" "))?;

    writeln!(out, "SHA256:")?;
    for entry in entries {
        writeln!(
            out,
            " {} {:>8} {}/binary-{}/Packages",
            entry
                .checksums
                .sha256
                .as_ref()
                .with_context(|| format!("missing sha256 for {}", entry.name))?,
            entry.size,
            entry.name,
            arch,
        )?;
    }
    writeln!(out, "SHA1:")?;
    for entry in entries {
        writeln!(
            out,
            " {} {:>8} {}/binary-{}/Packages",
            entry
                .checksums
                .sha1
                .as_ref()
                .with_context(|| format!("missing sha1 for {}", entry.name))?,
            entry.size,
            entry.name,
            arch,
        )?;
    }

    Ok(out)
}
