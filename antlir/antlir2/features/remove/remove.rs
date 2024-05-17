/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::PathInLayer;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = Remove;

#[derive(
    Debug,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
pub struct Remove {
    pub path: PathInLayer,
    pub must_exist: bool,
    pub must_be_empty: bool,
}

impl antlir2_depgraph_if::RequiresProvides for Remove {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(match self.must_exist {
            false => vec![],
            true => vec![Requirement::ordered(
                ItemKey::Path(self.path.to_owned()),
                Validator::Exists,
            )],
        })
    }
}

impl antlir2_compile::CompileFeature for Remove {
    #[tracing::instrument(name = "remove", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let path = ctx.dst_path(&self.path)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) => {
                if !self.must_exist && e.kind() == std::io::ErrorKind::NotFound {
                    trace!("'{}' did not exist", self.path.display());
                    Ok(())
                } else if e.kind() == std::io::ErrorKind::IsADirectory {
                    if self.must_be_empty {
                        std::fs::remove_dir(&path).map_err(antlir2_compile::Error::from)
                    } else {
                        std::fs::remove_dir_all(&path).map_err(antlir2_compile::Error::from)
                    }
                } else {
                    Err(e.into())
                }
            }
        }
    }
}
