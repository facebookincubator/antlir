/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use antlir2_depgraph::Graph;
use antlir2_depgraph_if::AnalyzedFeature;
use anyhow::Context;
use clap::Parser;
use json_arg::JsonFile;

use crate::Result;

#[derive(Parser, Debug)]
/// Process an image's dependency graph without building it
pub(crate) struct Depgraph {
    #[clap(long = "feature")]
    features: Vec<JsonFile<AnalyzedFeature>>,
    #[clap(long = "parent")]
    /// Path to depgraph for the parent layer
    parent: Option<PathBuf>,
    #[clap(long)]
    /// Add dynamically built items from this facts database
    add_built_items: Option<PathBuf>,
    #[clap(long)]
    db_out: PathBuf,
    #[clap(long)]
    topo_features_out: PathBuf,
}

impl Depgraph {
    #[tracing::instrument(name = "depgraph", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let parent = self.parent.as_deref().map(Graph::open).transpose()?;
        let mut depgraph = Graph::builder(parent)?;
        for f in self.features.into_iter().map(JsonFile::into_inner) {
            depgraph.add_feature(f)?;
        }
        let depgraph = depgraph.build()?;

        let features: Vec<_> = depgraph.pending_features()?.collect();
        let mut out = BufWriter::new(
            File::create(&self.topo_features_out)
                .context("while creating topological features file")?,
        );
        serde_json::to_writer(&mut out, &features)
            .context("while writing out topologically-sorted features")?;

        depgraph
            .write_to_file(&self.db_out)
            .context("while serializing graph to file")?;

        Ok(())
    }
}
