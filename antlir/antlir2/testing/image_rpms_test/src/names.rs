/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::PathBuf;

use antlir2_facts::fact::rpm::Rpm;
use antlir2_facts::RoDatabase;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
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
}

impl Names {
    pub fn run(self) -> Result<()> {
        let facts = RoDatabase::open(&self.facts_db).context("while opening facts db")?;
        let installed_names: BTreeSet<String> = facts
            .iter::<Rpm>()
            .context("while getting rpms")?
            .map(|r| r.name().to_owned())
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
