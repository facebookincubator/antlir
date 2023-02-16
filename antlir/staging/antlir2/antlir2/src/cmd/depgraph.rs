/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;
use std::path::PathBuf;

use antlir2_depgraph::Graph;
use anyhow::Context;
use buck_label::Label;
use clap::Parser;
use clap::ValueEnum;
use json_arg::Json;
use json_arg::JsonFile;
use serde::Deserialize;

use crate::Result;

#[derive(Parser, Debug)]
pub(crate) struct Depgraph {
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<features::Feature<'static>>>>,
    #[clap(long = "parent")]
    /// Path to depgraph for the parent layer
    parent: Option<Json<DependencyArg<'static>>>,
    #[clap(long = "dependency")]
    /// Path to depgraphs for dependencies
    dependencies: Vec<Json<DependencyArg<'static>>>,
    #[clap(long)]
    /// Add dynamically built items from this built image
    add_built_items: Option<PathBuf>,
    #[clap(value_enum)]
    output: Output,
    #[clap(long, default_value = "-")]
    out: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyArg<'a> {
    #[serde(borrow)]
    label: Label<'a>,
    graph_path: PathBuf,
}

impl<'a> DependencyArg<'a> {
    fn load_graph(&self) -> Result<Graph<'static>> {
        let mut deser = serde_json::Deserializer::from_reader(
            std::fs::File::open(&self.graph_path).with_context(|| {
                format!("while opening input file {}", self.graph_path.display())
            })?,
        );
        Graph::deserialize(&mut deser)
            .with_context(|| format!("while reading depgraph from {}", self.graph_path.display()))
            .map_err(crate::Error::from)
    }
}

#[derive(Debug, ValueEnum, Copy, Clone)]
enum Output {
    Dot,
    Json,
}

impl super::Subcommand for Depgraph {
    fn run(self) -> Result<()> {
        let parent = match self.parent {
            Some(a) => Some(a.load_graph()?),
            None => None,
        };
        let mut depgraph = Graph::builder(parent);
        for features in self.features {
            for f in features.into_inner() {
                depgraph.add_feature(f);
            }
        }
        for dep in &self.dependencies {
            depgraph.add_layer_dependency(dep.label.clone(), dep.load_graph()?);
        }
        let mut depgraph = depgraph.build()?;
        if let Some(dir) = &self.add_built_items {
            depgraph
                .populate_dynamic_items(dir)
                .context("while adding dynamically built items")?;
        }

        let mut out = stdio_path::create(&self.out).context("while opening output")?;

        match self.output {
            Output::Dot => {
                let dot = depgraph.to_dot();
                writeln!(out, "{:#?}", dot)
            }
            Output::Json => {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&depgraph).context("while serializing graph")?
                )
            }
        }
        .context("while writing output")?;
        Ok(())
    }
}
