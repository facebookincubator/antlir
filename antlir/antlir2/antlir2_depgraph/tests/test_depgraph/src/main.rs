/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_depgraph::Graph;
use antlir2_depgraph_if::AnalyzedFeature;
use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use regex::Regex;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long = "feature")]
    features: Vec<JsonFile<AnalyzedFeature>>,
    #[clap(long)]
    parent: Option<PathBuf>,
    #[clap(long)]
    error_regex: Regex,
}

fn build(args: Args) -> antlir2_depgraph::Result<Graph> {
    let parent = args.parent.map(Graph::open).transpose()?;
    let mut builder = Graph::builder(parent)?;
    for feature in args.features.into_iter().map(JsonFile::into_inner) {
        eprintln!("adding feature {feature:?}");
        builder.add_feature(feature)?;
    }
    builder.build()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let error_regex = args.error_regex.clone();
    match build(args) {
        Ok(_) => Err(anyhow!("graph built successfully but shouldn't have")),
        Err(err) => {
            if !error_regex.is_match(&err.to_string()) {
                Err(anyhow!("'{err}' did not match '{}'", error_regex))
            } else {
                Ok(())
            }
        }
    }
}
