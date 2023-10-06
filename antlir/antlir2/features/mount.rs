/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
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
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Mount {
    fn provides(&self) -> Result<Vec<Item<'static>>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement<'static>>, String> {
        let mut v = vec![Requirement::ordered(
            ItemKey::Path(self.mountpoint().to_owned().into()),
            Validator::FileType(match self.is_directory() {
                true => FileType::Directory,
                false => FileType::File,
            }),
        )];
        match self {
            Self::Layer(l) => v.push(Requirement::ordered(
                ItemKey::Layer(l.src.label.to_owned()),
                Validator::Exists,
            )),
            Self::Host(_) => (),
        }
        Ok(v)
    }
}

impl antlir2_compile::CompileFeature for Mount {
    fn compile(&self, _ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        Ok(())
    }
}
