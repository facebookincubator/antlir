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
use buck_label::Label;
use clap::Parser;
use json_arg::Json;
use json_arg::JsonFile;
use regex::Regex;
use serde::Deserialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    label: Label<'static>,
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<Feature<'static>>>>,
    #[clap(long)]
    parent: Option<JsonFile<Graph<'static>>>,
    #[clap(long = "image-dependency")]
    /// Path to depgraphs for image dependencies
    dependencies: Vec<JsonFile<Graph<'static>>>,
    #[clap(long)]
    expect: Json<Expect<'static>>,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
enum Expect<'a> {
    Err(antlir2_depgraph::Error<'a>),
    ErrorRegex(#[serde_as(as = "DisplayFromStr")] Regex),
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut builder = Graph::builder(args.label, args.parent.map(JsonFile::into_inner));
    for feature in args.features.into_iter().flat_map(JsonFile::into_inner) {
        eprintln!("adding feature {feature:?}");
        builder.add_feature(feature);
    }
    for dep in args.dependencies {
        builder.add_layer_dependency(dep.into_inner());
    }
    let builder_dot = builder.to_dot();
    // always print this for debuggability
    eprintln!("{builder_dot:#?}");
    let result = builder.build();
    match args.expect.into_inner() {
        Expect::Err(expect_err) => match result {
            Ok(g) => Err(anyhow!("graph built successfully: {:#?}", g.to_dot())),
            Err(err) => {
                similar_asserts::assert_eq!(err, expect_err);
                Ok(())
            }
        },
        Expect::ErrorRegex(err_re) => match result {
            Ok(g) => Err(anyhow!("graph built successfully: {:#?}", g.to_dot())),
            Err(err) => {
                if !err_re.is_match(&err.to_string()) {
                    Err(anyhow!("'{err}' did not match '{err_re}'"))
                } else {
                    Ok(())
                }
            }
        },
    }
}
