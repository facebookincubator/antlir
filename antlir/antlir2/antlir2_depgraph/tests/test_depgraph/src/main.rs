/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::PathBuf;

use antlir2_depgraph::Graph;
use antlir2_depgraph_if::AnalyzedFeature;
use antlir2_facts::RwDatabase;
use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use regex::Regex;
use tempfile::NamedTempFile;

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
    let mut tmp_db = NamedTempFile::new().expect("failed to create tmp db file");
    let db = match &args.parent {
        Some(parent) => {
            let mut parent = File::open(parent).expect("failed to open parent db");
            std::io::copy(&mut parent, &mut tmp_db).expect("failed to make copy of parent db");
            RwDatabase::open(&tmp_db).expect("failed to open tmp db")
        }
        None => RwDatabase::create(tmp_db.path()).expect("failed to open db"),
    };
    let mut builder = Graph::builder(db)?;
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
