/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_features as _;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = DotMeta;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct DotMeta {
    /// Unknown for local dev builds (in other words not going to fbpkg)
    build_info: Option<BuildInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
struct BuildInfo {
    /// SCM revision that this layer was built off of
    revision: Option<String>,
    /// Package identifier
    package: Option<String>,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for DotMeta {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(Default::default())
    }
}

impl antlir2_compile::CompileFeature for DotMeta {
    #[tracing::instrument(name = "dot_meta", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        std::fs::create_dir_all(ctx.dst_path("/.meta")?).context("while creating /.meta")?;
        std::fs::write(
            ctx.dst_path("/.meta/target")?,
            format!("{}\n", ctx.label().as_unconfigured()),
        )
        .context("while writing /.meta/target")?;
        if let Some(build_info) = &self.build_info {
            if let Some(rev) = &build_info.revision {
                std::fs::write(ctx.dst_path("/.meta/revision")?, format!("{rev}\n"))
                    .context("while writing /.meta/revision")?;
            }

            if let Some(package) = &build_info.package {
                #[cfg(facebook)]
                let package_filename = "/.meta/fbpkg";
                #[cfg(not(facebook))]
                let package_filename = "/.meta/package";

                std::fs::write(ctx.dst_path(package_filename)?, format!("{package}\n"))
                    .context("while writing package info")?;
            }
        }
        Ok(())
    }
}
