/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use antlir2_depgraph_if::AnalyzedFeature;
use antlir2_depgraph_if::RequiresProvides;
use antlir2_features::Feature;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;

mod plugin;
use plugin::FeatureWrapper;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    feature: JsonFile<Feature>,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let feature = FeatureWrapper(&args.feature);
    let requires = feature
        .requires()
        .map_err(Error::msg)
        .context("while determining feature requires")?;
    let provides = feature
        .provides()
        .map_err(Error::msg)
        .context("while determining feature provides")?;
    let analyzed_feature = AnalyzedFeature::new(args.feature.into_inner(), requires, provides);
    let mut out = BufWriter::new(File::create(&args.out).context("while opening output file")?);
    serde_json::to_writer(&mut out, &analyzed_feature).context("while serializing analysis")?;
    Ok(())
}
