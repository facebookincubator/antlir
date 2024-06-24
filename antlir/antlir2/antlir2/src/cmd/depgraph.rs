/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::Permissions;
use std::io::BufWriter;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use antlir2_depgraph::Graph;
use antlir2_depgraph_if::AnalyzedFeature;
use antlir2_facts::RwDatabase;
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
    db_out: PathBuf,
    #[clap(long)]
    topo_features_out: PathBuf,
}

impl Depgraph {
    #[tracing::instrument(name = "depgraph", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let db = match &self.parent {
            Some(parent) => {
                std::fs::copy(parent, &self.db_out).context("while copying parent db")?;
                std::fs::set_permissions(&self.db_out, Permissions::from_mode(0o644))
                    .context("while making db writable")?;
                RwDatabase::open(&self.db_out)
                    .with_context(|| format!("while opening db '{}'", self.db_out.display()))?
            }
            None => RwDatabase::create(&self.db_out)
                .with_context(|| format!("while creating db '{}'", self.db_out.display()))?,
        };
        let mut depgraph = Graph::builder(db)?;
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

        Ok(())
    }
}
