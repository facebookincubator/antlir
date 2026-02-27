/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::PathBuf;

use antlir2_facts::RoDatabase;
use antlir2_facts::fact::deb::Deb;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    facts_db: PathBuf,
    #[clap(long)]
    not_installed: bool,
    /// Package names to check
    names: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let facts = RoDatabase::open(&args.facts_db).context("while opening facts db")?;
    let installed_names: BTreeSet<String> = facts
        .iter::<Deb>()
        .context("while getting debs")?
        .map(|d| d.name().to_owned())
        .collect();

    if args.not_installed {
        let unexpected: Vec<&String> = args
            .names
            .iter()
            .filter(|n| installed_names.contains(n.as_str()))
            .collect();
        ensure!(
            unexpected.is_empty(),
            "Expected these debs to NOT be installed, but they were: {}",
            unexpected
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    } else {
        let missing: Vec<&String> = args
            .names
            .iter()
            .filter(|n| !installed_names.contains(n.as_str()))
            .collect();
        ensure!(
            missing.is_empty(),
            "Expected these debs to be installed, but they were not: {}",
            missing
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
