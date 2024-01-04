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
use json_arg::JsonFile;

use crate::Result;

#[derive(Parser, Debug)]
/// Process an image's dependency graph without building it
pub(crate) struct Depgraph {
    #[clap(long)]
    label: Label,
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<antlir2_features::Feature>>>,
    #[clap(long = "parent")]
    /// Path to depgraph for the parent layer
    parent: Option<JsonFile<Graph<'static>>>,
    #[clap(long = "image-dependency")]
    /// Path to depgraphs for image dependencies
    dependencies: Vec<JsonFile<Graph<'static>>>,
    #[clap(long)]
    /// Add dynamically built items from this built image
    add_built_items: Option<PathBuf>,
    #[clap(value_enum)]
    output: Output,
    #[clap(long, default_value = "-")]
    out: PathBuf,
    #[clap(long)]
    rootless: bool,
}

#[derive(Debug, ValueEnum, Copy, Clone)]
enum Output {
    Dot,
    Json,
}

impl Depgraph {
    #[tracing::instrument(name = "depgraph", skip(self))]
    pub(crate) fn run(self, rootless: antlir2_rootless::Rootless) -> Result<()> {
        // This naming is a little confusing, but basically `rootless` exists to
        // drop privileges when the process is invoked with `sudo`, and as such
        // is not used if the entire build is running solely as an unprivileged
        // user.
        let rootless = if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
            None
        } else {
            Some(rootless)
        };

        let mut depgraph = Graph::builder(self.label, self.parent.map(JsonFile::into_inner));
        for features in self.features {
            for f in features.into_inner() {
                depgraph.add_feature(f);
            }
        }
        for dep in self.dependencies {
            depgraph.add_layer_dependency(dep.into_inner());
        }
        let mut depgraph = depgraph.build()?;
        if let Some(dir) = &self.add_built_items {
            let _root_guard = rootless.map(|r| r.escalate()).transpose()?;
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
