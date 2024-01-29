/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use antlir2_facts::fact::rpm::Rpm;
use antlir2_facts::RoDatabase;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Serialize;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    facts_db: PathBuf,
    #[clap(long)]
    out: PathBuf,
}

#[derive(Debug, Serialize)]
struct Manifest {
    rpms: Vec<ManifestRpm>,
}

// TODO: do we *actually* need a different serialization representation or can
// we just dump the fact [Rpm] as-is?
// I have no idea what consumes these manifests today, so we'll just match the
// antlir1 behavior for now.
#[derive(Debug, Serialize)]
struct ManifestRpm {
    name: String,
    nevra: Nevra,
    patched_cves: Vec<String>,
    os: Option<String>,
    size: u64,
    source_rpm: String,
}

#[derive(Debug, Serialize)]
struct Nevra {
    name: String,
    #[serde(rename = "epochnum")]
    epoch: u64,
    version: String,
    release: String,
    arch: String,
}

impl From<Rpm<'_>> for ManifestRpm {
    fn from(rpm: Rpm<'_>) -> Self {
        Self {
            name: rpm.name().to_owned(),
            nevra: Nevra {
                name: rpm.name().to_owned(),
                epoch: rpm.epoch(),
                version: rpm.version().to_owned(),
                release: rpm.release().to_owned(),
                arch: rpm.arch().to_owned(),
            },
            patched_cves: rpm
                .patched_cves()
                .into_iter()
                .map(|cve| cve.to_owned())
                .collect(),
            os: rpm.os().map(|os| os.to_owned()),
            size: rpm.size(),
            source_rpm: rpm.source_rpm().to_owned(),
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let db =
        RoDatabase::open(&args.facts_db, Default::default()).context("while opening facts db")?;
    let mut out = BufWriter::new(File::create(&args.out).context("while creating output file")?);
    let rpms = db.iter::<Rpm>().map(ManifestRpm::from).collect();
    let manifests = Manifest { rpms };
    serde_json::to_writer(&mut out, &manifests).context("while serializing manifest")?;
    Ok(())
}
