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
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use r#impl::Feature;
use json_arg::JsonFile;

static_assertions::assert_impl_all!(
    Feature: antlir2_depgraph_if::RequiresProvides
);

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    feature: JsonFile<antlir2_features::Feature>,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let generic_feature = args.feature.into_inner();

    let this_feature: Feature = serde_json::from_value(generic_feature.data.clone())
        .context("while parsing as specific feature")?;
    let requires = this_feature
        .requires()
        .map_err(Error::msg)
        .context("while determining feature requires")?;
    let provides = this_feature
        .provides()
        .map_err(Error::msg)
        .context("while determining feature provides")?;

    let analyzed_feature = AnalyzedFeature::new(generic_feature, requires, provides);
    let mut out = BufWriter::new(File::create(&args.out).context("while opening output file")?);
    serde_json::to_writer(&mut out, &analyzed_feature).context("while serializing analysis")?;
    Ok(())
}
