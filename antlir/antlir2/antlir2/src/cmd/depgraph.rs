/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;

use antlir2_depgraph::Graph;
use antlir2_facts::RoDatabase;
use anyhow::Context;
use clap::Parser;
use json_arg::JsonFile;

use crate::Result;

#[derive(Parser, Debug)]
/// Process an image's dependency graph without building it
pub(crate) struct Depgraph {
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<HashSet<antlir2_features::Feature>>>,
    #[clap(long = "parent")]
    /// Path to depgraph for the parent layer
    parent: Option<PathBuf>,
    #[clap(long)]
    /// Add dynamically built items from this facts database
    add_built_items: Option<PathBuf>,
    #[clap(long)]
    out: PathBuf,
}

impl Depgraph {
    #[tracing::instrument(name = "depgraph", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let parent = self.parent.as_deref().map(Graph::open).transpose()?;
        let mut depgraph = Graph::builder(parent);
        for f in self.features.into_iter().flat_map(JsonFile::into_inner) {
            depgraph.add_feature(f);
        }
        let mut depgraph = depgraph.build()?;
        if let Some(path) = &self.add_built_items {
            let db = RoDatabase::open(path).context("while opening facts db")?;
            depgraph
                .populate_dynamic_items(&db)
                .context("while adding dynamically built items")?;
        }

        if let Some(parent) = &self.parent {
            let mut src = File::open(parent).context("while opening parent db")?;
            let mut out = File::create(&self.out).context("while creating output file")?;
            std::io::copy(&mut src, &mut out).context("while copying parent db")?;
        }

        depgraph
            .write_to_file(&self.out)
            .context("while serializing graph to file")?;
        Ok(())
    }
}
