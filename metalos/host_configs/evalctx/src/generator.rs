/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use metalos_host_configs::provisioning_config::ProvisioningConfig;
use shadow::ShadowFile;
use shadow::ShadowRecord;
use slog::info;
use slog::Logger;
use xattr::FileExt;

use crate::path::PathExt;
use crate::Error;
use crate::Result;

pub type Username = String;
pub type PWHash = String;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Output {
    pub files: Vec<File>,
    pub dirs: Vec<Dir>,
    pub pw_hashes: Option<BTreeMap<Username, PWHash>>,
    pub zero_files: Vec<ZeroFile>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Dir {
    pub path: PathBuf,
}

#[derive(PartialEq, Eq, Clone)]
pub struct File {
    pub path: PathBuf,
    pub contents: Vec<u8>,
    pub mode: u32,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ZeroFile {
    pub path: PathBuf,
    pub block_size_bytes: usize,
    pub block_count: u32,
    pub mode: u32,
}

impl std::fmt::Debug for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("File")
            .field("path", &self.path)
            .field("mode", &format!("{:#o}", &self.mode))
            .field(
                "contents",
                &std::str::from_utf8(&self.contents).unwrap_or("<binary data>"),
            )
            .finish()
    }
}

impl Output {
    pub fn apply(self, log: Logger, root: &Path) -> Result<()> {
        for dir in self.dirs {
            let dst = root.force_join(dir.path);
            info!(log, "Creating dir: {:?}", dst);
            fs::create_dir_all(dst).map_err(Error::Apply)?;
        }

        for file in self.files {
            let dst = root.force_join(file.path);
            Self::create_file(&log, dst, file.mode, |file_handle| {
                file_handle.write_all(&file.contents)
            })
            .map_err(Error::Apply)?;
        }

        for zero_file in self.zero_files {
            let dst = root.force_join(zero_file.path);
            Self::create_file(&log, dst, zero_file.mode, |file_handle| {
                let block: Vec<u8> = vec![0; zero_file.block_size_bytes];
                for _ in 0..zero_file.block_count {
                    file_handle.write_all(&block)?;
                }
                Ok(())
            })
            .map_err(Error::Apply)?;
        }

        if let Some(pw_hashes) = self.pw_hashes {
            Self::apply_pw_hashes(&log, pw_hashes, root).map_err(Error::PWHashError)?;
        }
        Ok(())
    }

    fn create_file<F>(log: &Logger, dst: PathBuf, mode: u32, writer: F) -> std::io::Result<()>
    where
        F: Fn(&mut fs::File) -> std::io::Result<()>,
    {
        info!(log, "Writing file: {:?}", dst);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::File::create(&dst)?;
        writer(&mut f)?;
        let mut perms = f.metadata()?.permissions();
        perms.set_mode(mode);
        f.set_permissions(perms)?;
        // Try to mark the file as metalos-generated, but swallow the error
        // if we can't. It's ok to fail silently here, because the only use
        // case for this xattr is debugging tools, and it's better to have
        // debug tools miss some files that come from generators, rather
        // than fail to apply configs entirely
        let _ = f.set_xattr("user.metalos.generator", &[1]);
        Ok(())
    }

    fn apply_pw_hashes(
        log: &Logger,
        pw_hashes: BTreeMap<Username, PWHash>,
        root: &Path,
    ) -> anyhow::Result<()> {
        let shadow_file = root.join("etc/shadow");
        let mut shadow =
            ShadowFile::from_file(&shadow_file).context("Failed to load existing shadows file")?;

        for (user, hash) in pw_hashes.into_iter() {
            info!(log, "Updating hash for {} to {}", user, hash);
            let record =
                ShadowRecord::new(user, hash).context("failed to create shadow record for")?;
            shadow.update_record(record);
        }
        info!(
            log,
            "Shadow file {:?} internal data: {:?}", shadow_file, shadow
        );

        let content = shadow
            .write_to_file(&shadow_file)
            .context("failed to write mutated shadow file")?;
        info!(
            log,
            "Writing shadow file {:?} with content: {:?}", shadow_file, content
        );

        Ok(())
    }
}

/// Abstract API that any MetalOS host config generator must implement.
pub trait Generator {
    fn name(&self) -> &str;

    fn eval(&self, host: &ProvisioningConfig) -> Result<Output>;
}

// This is explicitly implemented only for infallible functions, since
// Generators are conceptually infallible (but in practice, Starlark code may
// fail sometimes due to bugs like wrong types)
impl<F> Generator for F
where
    F: Fn(&ProvisioningConfig) -> Output,
{
    fn name(&self) -> &str {
        std::any::type_name::<F>()
    }

    fn eval(&self, prov: &ProvisioningConfig) -> Result<Output> {
        Ok(self(prov))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::Dir;
    use super::File;
    use super::Output;

    fn apply_one_output(output: Output) -> anyhow::Result<TempDir> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let tmp_dir = TempDir::new()?;
        output.apply(log, tmp_dir.path())?;
        Ok(tmp_dir)
    }

    #[test]
    fn apply_creates_dirs() -> anyhow::Result<()> {
        let tmp_dir = apply_one_output(Output {
            files: vec![],
            dirs: vec![Dir {
                path: "/a/b/c/d".into(),
            }],
            pw_hashes: None,
            zero_files: vec![],
        })?;
        let dir = std::fs::metadata(tmp_dir.path().join("a/b/c/d"))?;
        assert!(dir.is_dir());
        Ok(())
    }

    #[test]
    fn apply_creates_parent_dirs() -> anyhow::Result<()> {
        let tmp_dir = apply_one_output(Output {
            files: vec![File {
                path: "/a/b/c/d".into(),
                contents: "".into(),
                mode: 0o444,
            }],
            dirs: vec![],
            pw_hashes: None,
            zero_files: vec![],
        })?;
        let dir = std::fs::metadata(tmp_dir.path().join("a/b/c"))?;
        assert!(dir.is_dir());
        let file = std::fs::metadata(tmp_dir.path().join("a/b/c/d"))?;
        assert!(file.is_file());
        Ok(())
    }
}
