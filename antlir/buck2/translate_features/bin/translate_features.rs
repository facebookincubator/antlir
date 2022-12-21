/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use clap::Parser;
use features::Feature;
use serde::Deserialize;
use serde::Serialize;
use translate_features::IntoShape;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    label: Label<'static>,
    #[clap(long)]
    feature_json: Vec<PathBuf>,
    #[clap(long)]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut features: Vec<Feature> = Vec::new();
    for file in &args.feature_json {
        let mut deser = serde_json::Deserializer::from_reader(
            std::fs::File::open(file)
                .with_context(|| format!("while opening input file {}", file.display()))?,
        );
        let f = <Vec<Feature>>::deserialize(&mut deser)
            .with_context(|| format!("while reading features from {}", file.display()))?;
        features.extend(f);
    }
    let outf = std::fs::File::create(&args.output).context("while creating output file")?;
    let new_json = FeaturesFile {
        target: args.label,
        features: features.into_iter().map(|f| f.into_shape()).collect(),
    };
    serde_json::to_writer_pretty(outf, &new_json).context("while writing new json")?;

    Ok(())
}

#[derive(Serialize)]
pub struct FeaturesFile<'a> {
    target: Label<'a>,
    features: Vec<serde_json::Value>,
}
