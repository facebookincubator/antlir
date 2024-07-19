/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;

use antlir2_compile::util::copy_with_metadata;
use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::LayerInfo;
use antlir2_features::types::PathInLayer;
use anyhow::Context;
use extract::copy_dep;
use extract::so_dependencies;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = ExtractFromLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtractFromLayer {
    layer: LayerInfo,
    binaries: Vec<PathInLayer>,
}

/// In all the cases that we care about, a library will live under /lib64, but
/// this directory will be a symlink to /usr/lib64. To avoid build conflicts with
/// other image layers, replace it.
fn ensure_usr<'a>(path: &'a Path) -> Cow<'a, Path> {
    match path.starts_with("/lib") || path.starts_with("/lib64") {
        false => Cow::Borrowed(path),
        true => Cow::Owned(Path::new("/usr").join(path.strip_prefix("/").unwrap_or(path))),
    }
}

impl antlir2_depgraph_if::RequiresProvides for ExtractFromLayer {
    fn provides(&self) -> Result<Vec<Item>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(self
            .binaries
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
            .binaries
            .iter()
            .flat_map(|path| {
                vec![Requirement::ordered(
                    ItemKey::Path(path.parent().expect("dst always has parent").to_owned()),
                    Validator::FileType(FileType::Directory),
                )]
            })
            .collect())
    }
}

impl antlir2_compile::CompileFeature for ExtractFromLayer {
    #[tracing::instrument(name = "extract_from_layer", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let default_interpreter = Path::new(match ctx.target_arch() {
            Arch::X86_64 => "/usr/lib64/ld-linux-x86-64.so.2",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
        });
        let src_layer = self
            .layer
            .subvol_symlink
            .canonicalize()
            .context("while looking up abspath of src layer")?;
        trace!("extract root = {}", src_layer.display());
        let mut all_deps = HashSet::new();
        for binary in &self.binaries {
            let src = src_layer.join(binary.strip_prefix("/").unwrap_or(binary));
            let dst = ctx.dst_path(binary)?;

            let src_meta = std::fs::symlink_metadata(&src)
                .with_context(|| format!("while lstatting {}", src.display()))?;
            let real_src = if src_meta.is_symlink() {
                // If src is a symlink, the destination should also be
                // created as a symlink, and the target should be
                // processed as the real binary.

                let canonical_target = src
                    .canonicalize()
                    .with_context(|| format!("while canonicalizing {}", src.display()))?;

                if canonical_target
                    .components()
                    .any(|c| c.as_os_str() == OsStr::new("buck-out"))
                {
                    // There is only a single use case of antlir1's
                    // `extract.extract` that is using a buck-built binary
                    // installed into an image so this can be a hard failure and
                    // manually addressed when that user is migrated.
                    // TODO(T153572212) Reword above comment when antlir1 is dead
                    return Err(anyhow::anyhow!(
                        "{} looks like a buck-built binary ({}). You must use feature.extract_buck_binary instead",
                        src.display(),
                        canonical_target.display(),
                    ).into());
                }

                let canonical_target_rel = canonical_target
                    .strip_prefix(&src_layer)
                    .unwrap_or(canonical_target.as_path());
                let target_under_src = src_layer.join(
                    canonical_target_rel
                        .strip_prefix("/")
                        .unwrap_or(canonical_target.as_path()),
                );
                if !target_under_src.exists() {
                    return Err(anyhow::anyhow!(
                        "symlink target {} ({} under src_layer) does not actually exist",
                        canonical_target.display(),
                        target_under_src.display()
                    )
                    .into());
                }

                copy_with_metadata(
                    &target_under_src,
                    &ctx.dst_path(canonical_target_rel)?,
                    None,
                    None,
                )
                .context("while copying target_under_src to canonical_target_rel")?;

                // use the exact same link target when recreating the
                // symlinkg (in other words, the same "relativeness")
                let _ = std::fs::remove_file(&dst);
                let target = std::fs::read_link(&src).with_context(|| {
                    format!("while reading the link target of  {}", src.display())
                })?;

                std::os::unix::fs::symlink(&target, &dst).with_context(|| {
                    format!("while symlinking {} -> {}", dst.display(), target.display())
                })?;

                canonical_target
            } else {
                // if the binary is a regular file, copy it directly
                copy_with_metadata(&src, &dst, None, None)?;
                binary.to_owned()
            };

            all_deps.extend(
                so_dependencies(
                    real_src
                        .strip_prefix(&src_layer)
                        .unwrap_or(real_src.as_path()),
                    Some(&src_layer),
                    default_interpreter,
                )?
                .into_iter()
                .map(|path| ensure_usr(&path).to_path_buf()),
            );
        }
        let cwd = std::env::current_dir()?;
        for dep in all_deps {
            let path_in_src_layer = src_layer.join(dep.strip_prefix("/").unwrap_or(&dep));
            // If the dep path within the container is under the current
            // cwd (aka, the repo), we need to get the file out of the
            // host instead of the container.
            let dep_copy_path = if dep.starts_with(&cwd) {
                // As a good safety check, we also ensure that the file
                // does not exist inside the container, to prevent any
                // unintended extractions from the build host's
                // non-deterministic environment. This check should
                // never pass unless something about the build
                // environment setup wildly changes, so we should return
                // an error immediately in case it does.
                if path_in_src_layer.exists() {
                    return Err(anyhow::anyhow!(
                        "'{}' exists but it seems like we should get it from the host",
                        path_in_src_layer.display()
                    )
                    .into());
                }
                dep.clone()
            } else {
                path_in_src_layer
            };
            copy_dep(&dep_copy_path, &ctx.dst_path(&dep)?)?;
        }
        Ok(())
    }
}
