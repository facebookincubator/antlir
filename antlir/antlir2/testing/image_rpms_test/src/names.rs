/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::ensure;
use anyhow::Result;
use clap::Parser;
use image_test_lib::get_installed_rpms;

#[derive(Parser)]
pub(crate) struct Names {
    #[clap(long)]
    layer: PathBuf,
    path: PathBuf,
    #[clap(long)]
    not_installed: bool,
    #[clap(long)]
    /// Print details about installed rpms, don't run test
    print: bool,
}

impl Names {
    pub fn run(self) -> Result<()> {
        let installed = get_installed_rpms(self.layer)?;

        if self.print {
            let mut name_col_width = installed.keys().map(String::len).max().unwrap_or(0);
            if name_col_width < 20 {
                name_col_width = 20;
            }
            for (name, info) in installed {
                println!("{name:name_col_width$} {}", info.rpm_test_format());
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
        let installed_names: BTreeSet<String> = installed.keys().cloned().collect();
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
