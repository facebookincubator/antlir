/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fs::File;
use std::path::PathBuf;

use antlir2_features::mount::HostMount;
use antlir2_features::mount::LayerMount;
use antlir2_features::mount::Mount;
use antlir2_features::Data;
use antlir2_features::Feature;
use anyhow::Context;
use clap::Parser;
use json_arg::JsonFile;
use serde::Deserialize;

use crate::Result;

#[derive(Parser, Debug)]
/// Post-process mount features into a standalone JSON artifact
pub(crate) struct SerializeMounts {
    #[clap(long = "feature-json")]
    features: Vec<JsonFile<Vec<Feature<'static>>>>,
    #[clap(long)]
    parent: Option<JsonFile<BTreeSet<Mount<'static>>>>,
    #[clap(long)]
    out: PathBuf,
}

impl SerializeMounts {
    #[tracing::instrument(name = "serialize-mounts", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let mut mounts = match self.parent {
            Some(parent) => parent.into_inner(),
            None => BTreeSet::new(),
        };
        for features in self.features {
            for f in features.into_inner() {
                if let Data::Mount(m) = f.data {
                    // layer mounts may need recursive mounts
                    if let Mount::Layer(l) = &m {
                        let nested_mounts_file = File::open(&l.src.mounts).with_context(|| {
                            format!(
                                "while reading mounts for {} ({})",
                                l.src.label,
                                l.src.mounts.display()
                            )
                        })?;
                        let mut deser = serde_json::Deserializer::from_reader(nested_mounts_file);
                        let nested_mounts = <BTreeSet<Mount>>::deserialize(&mut deser)
                            .with_context(|| format!("while parsing mounts for {}", l.src.label))?;

                        for mount in nested_mounts {
                            mounts.insert(match mount {
                                Mount::Host(h) => Mount::Host(HostMount {
                                    mountpoint: l
                                        .mountpoint
                                        .join(
                                            h.mountpoint.strip_prefix("/").unwrap_or(&h.mountpoint),
                                        )
                                        .into(),
                                    src: h.src,
                                    is_directory: h.is_directory,
                                }),
                                Mount::Layer(l2) => Mount::Layer(LayerMount {
                                    mountpoint: l
                                        .mountpoint
                                        .join(
                                            l2.mountpoint
                                                .strip_prefix("/")
                                                .unwrap_or(&l2.mountpoint),
                                        )
                                        .into(),
                                    src: l2.src.clone(),
                                }),
                            });
                        }
                    }
                    mounts.insert(m);
                }
            }
        }
        let json = serde_json::to_string(&mounts).context("while serializing mounts")?;
        std::fs::write(&self.out, &json).context("while writing mounts.json")?;
        Ok(())
    }
}
