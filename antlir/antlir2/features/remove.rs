/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::PathInLayer;
use anyhow::Error;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = Remove;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Remove {
    pub path: PathInLayer,
    pub must_exist: bool,
}

impl<'f> antlir2_feature_impl::Feature<'f> for Remove {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        Ok(match self.must_exist {
            false => vec![],
            true => vec![Requirement::ordered(
                ItemKey::Path(self.path.to_owned().into()),
                Validator::Exists,
            )],
        })
    }

    #[tracing::instrument(name = "remove", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let path = ctx.dst_path(&self.path);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) => {
                if !self.must_exist && e.kind() == std::io::ErrorKind::NotFound {
                    trace!("'{}' did not exist", self.path.display());
                    Ok(())
                } else if e.kind() == std::io::ErrorKind::IsADirectory {
                    std::fs::remove_dir_all(&path).map_err(Error::from)
                } else {
                    Err(e.into())
                }
            }
        }
    }
}
