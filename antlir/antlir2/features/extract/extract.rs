/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs::File;
use std::hash::Hasher;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_compile::util::copy_with_metadata;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_features::types::PathInLayer;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Deserializer;
use serde::de::Error as _;
use tracing::trace;
use twox_hash::XxHash64;

pub type Feature = Extract;

/// An entry in the extract manifest describing a file to install.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum ManifestEntry {
    /// Copy a file from libs_dir to an absolute path in the image
    File { src_relpath: PathBuf, dst: PathBuf },
    /// Create a symlink at `link` pointing to `target`
    Symlink { link: PathBuf, target: PathBuf },
}

/// A manifest of files to install in the image, produced by an analysis action
/// and consumed by the extract feature compile step.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Manifest(pub BTreeSet<ManifestEntry>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Extract {
    pub provides: Vec<PathInLayer>,
    pub libs: Libs,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Libs {
    #[serde(deserialize_with = "Libs::deserialize_manifest_file")]
    manifest: Manifest,
    libs_dir: PathBuf,
}

impl Libs {
    fn deserialize_manifest_file<'de, D>(deserializer: D) -> Result<Manifest, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        let f =
            BufReader::new(File::open(&path).map_err(|e| {
                D::Error::custom(format!("failed to open {}: {e}", path.display()))
            })?);
        serde_json::from_reader(f).map_err(D::Error::custom)
    }
}

impl antlir2_depgraph_if::RequiresProvides for Extract {
    fn provides(&self) -> Result<Vec<Item>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(self
            .provides
            .iter()
            .map(|path| {
                Item::Path(PathItem::Entry(FsEntry {
                    path: path.to_owned(),
                    file_type: FileType::File,
                    mode: 0o555,
                }))
            })
            .collect())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(self
            .provides
            .iter()
            .map(|path| {
                Requirement::ordered(
                    ItemKey::Path(path.parent().expect("dst always has parent").to_owned()),
                    Validator::FileType(FileType::Directory),
                )
            })
            .collect())
    }
}

impl antlir2_compile::CompileFeature for Extract {
    #[tracing::instrument(name = "extract", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        for entry in &self.libs.manifest.0 {
            match entry {
                ManifestEntry::File { src_relpath, dst } => {
                    trace!("copying {} -> {}", src_relpath.display(), dst.display());
                    copy_dep(&self.libs.libs_dir.join(src_relpath), &ctx.dst_path(dst)?)?;
                }
                ManifestEntry::Symlink { link, target } => {
                    trace!("symlinking {} -> {}", link.display(), target.display());
                    let dst = ctx.dst_path(link)?;
                    let _ = std::fs::remove_file(&dst);
                    std::os::unix::fs::symlink(target, &dst)?;
                }
            }
        }
        Ok(())
    }
}

#[tracing::instrument(err, ret)]
pub fn copy_dep(dep: &Path, dst: &Path) -> Result<()> {
    // create the destination directory tree based on permissions in the source
    if !dst.parent().expect("dst always has parent").exists() {
        for dir in dst
            .parent()
            .expect("dst always has parent")
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if !dir.exists() {
                trace!("creating parent directory {}", dir.display());
                std::fs::create_dir(dir)?;
            }
        }
    }
    trace!("getting metadata of {}", dep.display());
    let metadata = std::fs::symlink_metadata(dep)
        .with_context(|| format!("while statting '{}'", dep.display()))?;
    trace!("stat of {}: {metadata:?}", dep.display());
    // Thar be dragons. Copying symlinks is probably _never_ what we want - for
    // extracting binaries we want the contents of these dependencies
    let dep: Cow<Path> = if metadata.is_symlink() {
        Cow::Owned(
            std::fs::canonicalize(dep)
                .with_context(|| format!("while canonicalizing symlink dep '{}'", dep.display()))?,
        )
    } else {
        Cow::Borrowed(dep)
    };
    // If the destination file already exists, make sure it's exactly the same
    // as what we're about to copy, to prevent issues like
    // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
    if dst.exists() &&
    // We don't want to compare against files in /usr/local/fbcode, because the
    // different RE containers these are pulled from might have slightly
    // different versions of the fbcode platform, but the same thing could
    // easily happen for builds so just let it slide.
    !dep.display().to_string().contains("/usr/local/fbcode/")
    {
        let dst_contents = std::fs::read(dst)
            .with_context(|| format!("while reading already-installed '{}'", dst.display()))?;
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&dst_contents);
        let pre_existing_hash = hasher.finish();

        let src_contents = std::fs::read(&dep)
            .with_context(|| format!("while reading potentially new dep '{}'", dep.display()))?;
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&src_contents);
        let new_src_hash = hasher.finish();

        trace!(
            "hashed {} (existing = {}, new = {})",
            dst.display(),
            pre_existing_hash,
            new_src_hash
        );

        if pre_existing_hash != new_src_hash {
            return Err(anyhow::anyhow!(
                "extract conflicts with existing file at {}",
                dst.display()
            ));
        }
    } else {
        copy_with_metadata().src(&dep).dst(dst).call()?
    }
    Ok(())
}
