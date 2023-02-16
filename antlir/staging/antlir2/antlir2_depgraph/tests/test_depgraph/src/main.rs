/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_depgraph::Graph;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use features::Feature;
use json_arg::Json;
use json_arg::JsonFile;
use serde::Deserialize;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<Feature<'static>>>>,
    #[clap(long)]
    parent: Option<JsonFile<Graph<'static>>>,
    #[clap(long)]
    expect: Json<Expect<'static>>,
    #[clap(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
enum Expect<'a> {
    Ok,
    Err(antlir2_depgraph::Error<'a>),
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut builder = Graph::builder(args.parent.map(JsonFile::into_inner));
    for feature in args.features.into_iter().flat_map(JsonFile::into_inner) {
        eprintln!("adding feature {feature:?}");
        builder.add_feature(feature);
    }
    let builder_dot = builder.to_dot();
    // always print this for debuggability
    eprintln!("{builder_dot:#?}");
    let result = builder.build();
    match args.expect.into_inner() {
        Expect::Ok => match result {
            Ok(g) => {
                let path = args.out.context("--out must be set if --expect=ok")?;
                let parent = path.parent().context("must have parent")?;
                if !parent.exists() {
                    std::fs::create_dir(parent).context("while creating parent dir")?;
                }
                let f = stdio_path::create(&path)
                    .with_context(|| format!("while opening output file '{}'", path.display()))?;
                serde_json::to_writer_pretty(f, &g).context("while writing out graph json")?;
                Ok(())
            }
            Err(err) => Err(anyhow!("graph failed to build: {err}")),
        },
        Expect::Err(expect_err) => match result {
            Ok(g) => Err(anyhow!("graph built successfully: {:#?}", g.to_dot())),
            Err(err) => {
                similar_asserts::assert_eq!(err, expect_err);
                Ok(())
            }
        },
    }
}
