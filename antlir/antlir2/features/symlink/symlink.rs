/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::PathInLayer;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = Symlink;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Symlink {
    pub link: PathInLayer,
    pub target: PathInLayer,
    pub is_directory: bool,
    pub unsafe_dangling_symlink: bool,
}

impl antlir2_depgraph_if::RequiresProvides for Symlink {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(Path::Symlink {
            link: self.link.to_owned(),
            target: self.target.to_owned(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut requires = vec![Requirement::ordered(
            ItemKey::Path(
                self.link
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("/"))
                    .to_owned(),
            ),
            Validator::FileType(FileType::Directory),
        )];
        // target may be a relative path, in which
        // case we need to resolve it relative to
        // the link
        let absolute_target = match self.target.is_absolute() {
            true => self.target.to_owned(),
            false => {
                let mut absolute_target = self
                    .link
                    .parent()
                    .expect("the link cannot itself be /")
                    .to_owned();

                for component in self.target.components() {
                    match component {
                        std::path::Component::Normal(c) => absolute_target.push(c),
                        std::path::Component::RootDir => absolute_target.clear(),
                        std::path::Component::CurDir => {}
                        std::path::Component::ParentDir => {
                            absolute_target.pop();
                        }
                        std::path::Component::Prefix(_) => {
                            unimplemented!("Windows-style paths are not supported");
                        }
                    };
                }
                absolute_target
            }
        };
        // Allow an image author to create a symlink to certain files without verifying
        // that they exist, when the target indicates that the author knows what
        // they're doing.
        // /dev/null will reasonably always exist
        // /run will almost certainly be a tmpfs at runtime (but
        // TODO(T152984868) to ensure that)
        if !self.unsafe_dangling_symlink
            && absolute_target != std::path::Path::new("/dev/null")
            && !absolute_target.starts_with("/run")
        {
            // the symlink action itself does not really care if the target
            // exists yet or if it will be created later in the run, but any
            // features that depend on this symlink do, so just always order the
            // symlink after its target
            requires.push(Requirement::ordered(
                ItemKey::Path(absolute_target),
                Validator::FileType(match self.is_directory {
                    true => FileType::Directory,
                    false => FileType::File,
                }),
            ));
        }
        Ok(requires)
    }
}

impl antlir2_compile::CompileFeature for Symlink {
    #[tracing::instrument(name = "symlink", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let link = ctx.dst_path(&self.link)?;
        if let Ok(target) = std::fs::read_link(&link) {
            // the depgraph should have already ensured that it points to the
            // right location, but it can't hurt to check again
            if target != self.target {
                return Err(anyhow::anyhow!(
                    "symlink {} already exists, but points to {}, not {}",
                    self.link.display(),
                    target.display(),
                    self.target.display()
                )
                .into());
            } else {
                tracing::debug!("symlink already exists");
                return Ok(());
            }
        }
        trace!("symlinking {} -> {}", link.display(), self.target.display());
        std::os::unix::fs::symlink(&self.target, &link)?;
        Ok(())
    }
}
