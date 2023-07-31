/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::PathInLayer;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Symlink;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Symlink {
    pub link: PathInLayer,
    pub target: PathInLayer,
    pub is_directory: bool,
}

impl<'f> antlir2_feature_impl::Feature<'f> for Symlink {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(vec![Item::Path(Path::Symlink {
            link: self.link.to_owned().into(),
            target: self.target.to_owned().into(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        let mut requires = vec![Requirement::ordered(
            ItemKey::Path(
                self.link
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("/"))
                    .to_owned()
                    .into(),
            ),
            Validator::FileType(FileType::Directory),
        )];
        // target may be a relative path, in which
        // case we need to resolve it relative to
        // the link
        let absolute_target = match self.target.is_absolute() {
            true => self.target.to_owned(),
            false => self
                .link
                .parent()
                .expect("the link cannot itself be /")
                .join(&self.target),
        };
        // Allow an image author to create a symlink to certain files without verifying
        // that they exist, when the target indicates that the author knows what
        // they're doing.
        // /dev/null will reasonably always exist
        // /run will almost certainly be a tmpfs at runtime (but
        // TODO(T152984868) to ensure that)
        if absolute_target != std::path::Path::new("/dev/null")
            && !absolute_target.starts_with("/run")
        {
            requires.push(Requirement::unordered(
                ItemKey::Path(absolute_target.into()),
                Validator::FileType(match self.is_directory {
                    true => FileType::Directory,
                    false => FileType::File,
                }),
            ));
        }
        Ok(requires)
    }

    #[tracing::instrument(name = "symlink", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        // Unlike antlir1, we don't have to do all the paranoid checking,
        // because the new depgraph will have done it all for us already.
        // I am also choosing to do preserve absolute symlinks if that's what
        // the user asked for, since it's more intuitive when the image is
        // installed somewhere and used as a rootfs, and doing things "inside"
        // the image without actually doing some form of chroot is super broken
        // anyway.
        if std::fs::symlink_metadata(ctx.dst_path(&self.link)).is_ok() {
            // the depgraph already ensured that it points to the right location
            tracing::debug!("symlink already exists");
            return Ok(());
        }
        std::os::unix::fs::symlink(&self.target, ctx.dst_path(&self.link))?;
        Ok(())
    }
}
