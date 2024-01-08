/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::Permissions;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use nix::sys::stat::SFlag;
use nix::unistd::Gid;
use nix::unistd::Uid;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;
use walkdir::DirEntryExt;
use walkdir::WalkDir;

#[derive(Debug)]
/// [CasDir] is a directory that can represent a filesystem with all of its
/// contents and metadata in a directory that is still owned by a single,
/// unprivileged user.
///
/// All of the file contents and directory structure are represented inside the
/// [CasDir], but ownership and mode is only preserved in the json
/// [CasDirManifest] - all files are owned by the unprivileged build user.
pub struct CasDir {
    path: PathBuf,
    contents_dir: PathBuf,
    manifest: Manifest,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    paths: BTreeMap<PathBuf, PathEntry>,
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
struct PathEntry {
    ino: u64,
    uid: u32,
    gid: u32,
    mode: u32,
}

#[derive(Debug, Copy, Clone)]
pub struct CasDirOpts {
    uid: Uid,
    gid: Gid,
}

impl Default for CasDirOpts {
    fn default() -> Self {
        Self {
            uid: Uid::current(),
            gid: Gid::current(),
        }
    }
}

impl CasDirOpts {
    pub fn uid(mut self, uid: impl Into<Uid>) -> Self {
        self.uid = uid.into();
        self
    }

    pub fn gid(mut self, gid: impl Into<Gid>) -> Self {
        self.gid = gid.into();
        self
    }
}

impl CasDir {
    /// Create a [CasDir] from a regular directory. For performance, root and
    /// dst should really be on the same (Copy-on-Write) filesystem. Otherwise
    /// this will still work, but will have to actually copy file contents and
    /// thus will be slower.
    pub fn dehydrate(root: impl AsRef<Path>, dst: PathBuf, opts: CasDirOpts) -> Result<Self> {
        let root = root.as_ref();
        std::fs::create_dir(&dst).context("while creating output directory")?;
        std::os::unix::fs::lchown(&dst, Some(opts.uid.as_raw()), Some(opts.gid.as_raw()))
            .context("while chowning dst")?;
        let contents_dir = dst.join("contents");
        std::fs::create_dir(&contents_dir).context("while creating contents directory")?;
        std::os::unix::fs::lchown(
            &contents_dir,
            Some(opts.uid.as_raw()),
            Some(opts.gid.as_raw()),
        )
        .context("while chowning dst")?;
        let mut manifest = Manifest::default();
        let mut inodes = HashMap::new();
        for entry in WalkDir::new(root) {
            let entry = entry.context("while walking root")?;
            let relpath = entry
                .path()
                .strip_prefix(root)
                .expect("must be relative to root");
            if relpath == Path::new("") {
                continue;
            }
            let meta = entry.metadata().context("while statting")?;
            trace!(
                "copying {} ({}) {meta:?}",
                relpath.display(),
                entry.path().display()
            );
            let dst_path = contents_dir.join(relpath);
            if entry.file_type().is_dir() {
                std::fs::create_dir(&dst_path).context("while creating dir in contents")?;
                std::fs::set_permissions(&dst_path, Permissions::from_mode(0o755))
                    .context("while chmodding contents dir")?;
            } else if entry.file_type().is_symlink() {
                let target = std::fs::read_link(entry.path()).context("while reading symlink")?;
                std::os::unix::fs::symlink(target, &dst_path)
                    .context("while making symlink in contents")?;
            } else if entry.file_type().is_file() {
                if let Some(already_copied) = inodes.get(&entry.ino()) {
                    std::fs::hard_link(already_copied, &dst_path)
                        .context("while hardlinking copy")?;
                } else {
                    std::fs::copy(entry.path(), &dst_path).with_context(|| {
                        format!("while copying {} out of layer", entry.path().display())
                    })?;
                    std::fs::set_permissions(&dst_path, Permissions::from_mode(0o444))
                        .context("while setting permissions")?;
                    inodes.insert(entry.ino(), dst_path.clone());
                }
            } else {
                unreachable!();
            }
            std::os::unix::fs::lchown(dst_path, Some(opts.uid.as_raw()), Some(opts.gid.as_raw()))
                .context("while chowning copy")?;
            manifest.paths.insert(
                relpath.to_path_buf(),
                PathEntry {
                    ino: meta.ino(),
                    uid: meta.uid(),
                    gid: meta.gid(),
                    mode: meta.mode(),
                },
            );
        }
        std::fs::write(
            dst.join("manifest.json"),
            serde_json::to_vec(&manifest).context("while serializing manifest")?,
        )
        .context("while writing manifest")?;
        std::os::unix::fs::lchown(
            dst.join("manifest.json"),
            Some(opts.uid.as_raw()),
            Some(opts.gid.as_raw()),
        )
        .context("while chowning manifest")?;
        Ok(Self {
            path: dst,
            contents_dir,
            manifest,
        })
    }

    /// Open a previously [dehydrate]d [CasDir].
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let manifest = std::fs::read_to_string(root.join("manifest.json"))
            .context("while reading manifest.json")?;
        let manifest: Manifest =
            serde_json::from_str(&manifest).context("while deserializing manifest.json")?;
        let contents_dir = root.join("contents");
        Ok(Self {
            path: root.to_owned(),
            contents_dir,
            manifest,
        })
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    /// Hydrate the [CasDir] into a regular directory with all the filesystem
    /// metadata being reproduced. The directory must already exist and be
    /// empty.
    pub fn hydrate_into(&self, dst: impl AsRef<Path>) -> Result<()> {
        let dst = dst.as_ref();
        ensure!(
            std::fs::read_dir(dst)
                .context("while ensuring output directory is empty")?
                .count()
                == 0,
            "output directory '{}' was not empty",
            dst.display()
        );

        let mut new_inos = HashMap::<u64, PathBuf>::new();
        for (relpath, entry) in &self.manifest.paths {
            let received_path = dst.join(relpath);
            if let Some(new_path) = new_inos.get(&entry.ino) {
                std::fs::hard_link(new_path, &received_path).with_context(|| {
                    format!(
                        "while hard linking {} -> {}",
                        received_path.display(),
                        new_path.display()
                    )
                })?;
                continue;
            }
            let sflag = SFlag::from_bits_truncate(entry.mode);
            if sflag.contains(SFlag::S_IFDIR) {
                std::fs::create_dir(&received_path).with_context(|| {
                    format!("while creating directory {}", received_path.display())
                })?;
                std::fs::set_permissions(&received_path, Permissions::from_mode(entry.mode))
                    .with_context(|| format!("while chmodding {}", received_path.display()))?;
            } else if sflag.contains(SFlag::S_IFLNK) {
                let target =
                    std::fs::read_link(self.contents_dir.join(relpath)).with_context(|| {
                        format!(
                            "while reading link {}",
                            self.contents_dir.join(relpath).display()
                        )
                    })?;
                std::os::unix::fs::symlink(&target, &received_path).with_context(|| {
                    format!(
                        "while symlinking {} -> {}",
                        received_path.display(),
                        target.display()
                    )
                })?;
            } else {
                std::fs::copy(self.contents_dir.join(relpath), &received_path).with_context(
                    || {
                        format!(
                            "while copying file {} -> {}",
                            received_path.display(),
                            self.contents_dir.join(relpath).display()
                        )
                    },
                )?;
                std::fs::set_permissions(&received_path, Permissions::from_mode(entry.mode))
                    .with_context(|| format!("while chmodding {}", received_path.display()))?;
            }
            std::os::unix::fs::lchown(&received_path, Some(entry.uid), Some(entry.gid))
                .with_context(|| format!("while chowning {}", received_path.display()))?;
            new_inos.insert(entry.ino, received_path);
        }
        Ok(())
    }
}

#[cfg(all(test, image_test))]
mod tests {
    use super::*;

    fn dehydrate_and_hydrate(opts: CasDirOpts) -> CasDir {
        let _cas_dir = CasDir::dehydrate("/src", "/cas_dir".into(), opts)
            .expect("failed to dehydrate from regular directory");
        // unnecessary, but proves that `open()` works
        let cas_dir = CasDir::open("/cas_dir").expect("failed to open hydrated directory");
        cas_dir
            .hydrate_into("/hydrated")
            .expect("failed to hydrate");
        cas_dir
    }

    #[test]
    fn hardlinks() {
        // antlir2 doesn't provide a hardlink api, so do it here
        std::fs::hard_link("/src/TARGETS", "/src/TARGETS.link").expect("failed to hardlink");
        let cas_dir = dehydrate_and_hydrate(CasDirOpts::default());
        {
            let ino = std::fs::metadata(cas_dir.contents_dir.join("TARGETS"))
                .expect("failed to stat TARGETS")
                .ino();
            let ino2 = std::fs::metadata(cas_dir.contents_dir.join("TARGETS.link"))
                .expect("failed to stat TARGETS.link")
                .ino();
            assert_eq!(
                ino, ino2,
                "hardlinks should have been preserved during dehydration"
            );
        }
        {
            let ino = std::fs::metadata("/hydrated/TARGETS")
                .expect("failed to stat /hydrated/TARGETS")
                .ino();
            let ino2 = std::fs::metadata("/hydrated/TARGETS.link")
                .expect("failed to stat /hydrated/TARGETS.link")
                .ino();
            assert_eq!(
                ino, ino2,
                "hardlinks should have been preserved during hydration"
            );
        }
    }

    #[test]
    fn symlink() {
        // antlir2 doesn't provide a relative symlink api, so do it here
        std::os::unix::fs::symlink("TARGETS", "/src/subdir/TARGETS.symlink.rel")
            .expect("failed to make relative symlink");
        let cas_dir = dehydrate_and_hydrate(CasDirOpts::default());
        {
            let target =
                std::fs::read_link(cas_dir.contents_dir.join("subdir/TARGETS.symlink.abs"))
                    .expect("failed to read abs link");
            assert_eq!(
                Path::new("/src/subdir/TARGETS"),
                target,
                "abs symlink should have been preserved during dehydration"
            );
            let target =
                std::fs::read_link(cas_dir.contents_dir.join("subdir/TARGETS.symlink.rel"))
                    .expect("failed to read rel link");
            assert_eq!(
                Path::new("TARGETS"),
                target,
                "rel symlink should have been preserved during dehydration"
            );
        }
        {
            let target = std::fs::read_link("/hydrated/subdir/TARGETS.symlink.abs")
                .expect("failed to read hydrated abs link");
            assert_eq!(
                Path::new("/src/subdir/TARGETS"),
                target,
                "abs symlink should have been preserved during hydration"
            );
            let target = std::fs::read_link("/hydrated/subdir/TARGETS.symlink.rel")
                .expect("failed to read hydrated rel link");
            assert_eq!(
                Path::new("TARGETS"),
                target,
                "rel symlink should have been preserved during hydration"
            );
        }
    }
}
