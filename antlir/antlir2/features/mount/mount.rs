/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::LayerInfo;
use antlir2_features::types::PathInLayer;
use anyhow::Result;
use serde::de::Error;
use serde::Deserialize;
use serde::Serialize;
use tracing as _;

pub type Feature = Mount;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount {
    Host(HostMount),
    Layer(LayerMount),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'de> Deserialize<'de> for Mount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct MountStruct {
            host: Option<HostMount>,
            layer: Option<LayerMount>,
        }

        MountStruct::deserialize(deserializer).and_then(|s| match (s.host, s.layer) {
            (Some(v), None) => Ok(Self::Host(v)),
            (None, Some(v)) => Ok(Self::Layer(v)),
            (_, _) => Err(D::Error::custom("exactly one of {host, layer} must be set")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct HostMount {
    pub mountpoint: PathInLayer,
    pub is_directory: bool,
    pub src: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LayerMount {
    pub mountpoint: PathInLayer,
    pub src: LayerInfo,
}

impl Mount {
    pub fn mountpoint(&self) -> &PathInLayer {
        match self {
            Self::Host(h) => &h.mountpoint,
            Self::Layer(l) => &l.mountpoint,
        }
    }

    pub fn is_directory(&self) -> bool {
        match self {
            Self::Layer(_) => true,
            Self::Host(h) => h.is_directory,
        }
    }

    fn mode(&self) -> u32 {
        match self {
            Self::Layer(_) => 0o555,
            Self::Host(h) => match h.is_directory {
                true => 0o555,
                false => 0o444,
            },
        }
    }
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Mount {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(Path::Mount(item::Mount {
            path: self.mountpoint().clone().into(),
            file_type: FileType::Directory,
            mode: self.mode(),
            source_description: format!("{self:#?}").into(),
        }))])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(vec![Requirement::ordered(
            ItemKey::Path(
                self.mountpoint()
                    .parent()
                    .unwrap_or(std::path::Path::new("/"))
                    .to_owned(),
            ),
            Validator::FileType(FileType::Directory),
        )])
    }
}

impl antlir2_compile::CompileFeature for Mount {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        match self.is_directory() {
            true => {
                std::fs::create_dir(ctx.dst_path(self.mountpoint())?)?;
            }
            false => {
                std::fs::File::create(ctx.dst_path(self.mountpoint())?)?;
            }
        }
        Ok(())
    }
}
