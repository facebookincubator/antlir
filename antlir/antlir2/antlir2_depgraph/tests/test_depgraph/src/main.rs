/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_depgraph::Graph;
use antlir2_features::Feature;
use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use regex::Regex;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<Feature>>>,
    #[clap(long)]
    parent: Option<JsonFile<Graph>>,
    #[clap(long)]
    error_regex: Regex,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut builder = Graph::builder(args.parent.map(JsonFile::into_inner));
    for feature in args.features.into_iter().flat_map(JsonFile::into_inner) {
        eprintln!("adding feature {feature:?}");
        builder.add_feature(feature);
    }
    let result = builder.build();
    match result {
        Ok(_) => Err(anyhow!("graph built successfully but shouldn't have")),
        Err(err) => {
            if !args.error_regex.is_match(&err.to_string()) {
                Err(anyhow!("'{err}' did not match '{}'", args.error_regex))
            } else {
                Ok(())
            }
        }
    }
}
