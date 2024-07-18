/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::ffi::OsString;
use std::fs::File;
use std::io::BufWriter;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use nix::mount::mount;
use nix::mount::umount;
use nix::mount::MsFlags;
use tracing::error;
use tracing::trace;

mod buck;
mod data_dir;
mod manifest;
mod scratch;
pub use buck::OverlayFs as BuckModel;
use scratch::Scratch;

use crate::manifest::Manifest;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error("Error parsing manifest: {0}")]
    Manifest(serde_json::Error),
    #[error("Error mounting overlayfs: {0}")]
    Mount(std::io::Error),
    #[error("Error preparing scratch directory: {0:#?}")]
    ScratchSetup(anyhow::Error),
    #[error("Error dehydrating overlayfs delta: {0:#?}")]
    Dehydrate(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct Opts {
    model: BuckModel,
    #[builder(default=default_scratch_root(), setter(strip_option))]
    scratch_root: Option<PathBuf>,
}

fn default_scratch_root() -> Option<PathBuf> {
    std::env::var_os("BUCK_SCRATCH_PATH").map(PathBuf::from)
}

#[derive(Debug)]
pub struct OverlayFs {
    model: BuckModel,
    scratch: Scratch,
}

impl OverlayFs {
    pub fn mount(opts: Opts) -> Result<Self> {
        let scratch = Scratch::setup(
            opts.scratch_root
                .context("scratch_root not set and BUCK_SCRATCH_PATH env var missing")
                .map_err(Error::ScratchSetup)?,
            &opts.model.layers,
        )
        .map_err(Error::ScratchSetup)?;
        if opts.model.top.data_dir.exists() {
            data_dir::unmangle(&opts.model.top.data_dir, scratch.upperdir())
                .context("while unmangling upper dir")
                .map_err(Error::ScratchSetup)?;
            let manifest = std::fs::read(&opts.model.top.manifest)
                .context("while reading top manifest")
                .map_err(Error::ScratchSetup)?;
            let manifest: Manifest = serde_json::from_slice(&manifest)
                .context("while parsing top manifest")
                .map_err(Error::ScratchSetup)?;
            manifest
                .fix_directory(scratch.upperdir())
                .context("while fixing upper dir metadata")
                .map_err(Error::ScratchSetup)?;
        }

        let mut options = OsString::from("userxattr,uuid=off");
        options.push(",upperdir=");
        options.push(scratch.upperdir());
        options.push(",workdir=");
        options.push(scratch.workdir());
        let mut lowerdirs = OsString::new();
        let mut lowerdirs_iter = scratch.lowerdirs().peekable();
        while let Some(lowerdir) = lowerdirs_iter.next() {
            lowerdirs.push(lowerdir);
            if lowerdirs_iter.peek().is_some() {
                lowerdirs.push(":");
            }
        }
        drop(lowerdirs_iter);
        if !lowerdirs.is_empty() {
            options.push(",lowerdir=");
            options.push(lowerdirs);
        }
        trace!(
            "mounting at {} with options {}",
            scratch.mountpoint().display(),
            String::from_utf8_lossy(options.as_bytes())
        );
        mount(
            Some("overlay"),
            scratch.mountpoint(),
            Some("overlay"),
            MsFlags::empty(),
            Some(options.as_os_str()),
        )
        .map_err(std::io::Error::from)
        .map_err(Error::Mount)?;
        Ok(Self {
            scratch,
            model: opts.model,
        })
    }

    pub fn mountpoint(&self) -> &Path {
        self.scratch.mountpoint()
    }

    pub fn finalize(self) -> Result<()> {
        umount(self.mountpoint())
            .map_err(std::io::Error::from)
            .map_err(Error::Mount)?;
        let manifest =
            Manifest::from_directory(self.scratch.upperdir()).map_err(Error::Dehydrate)?;
        let mut f = BufWriter::new(
            File::create(&self.model.top.manifest)
                .context("while creating manifest output file")
                .map_err(Error::Dehydrate)?,
        );
        serde_json::to_writer_pretty(&mut f, &manifest)
            .context("while serializing manifest output")
            .map_err(Error::Dehydrate)?;
        data_dir::mangle(self.scratch.upperdir(), &self.model.top.data_dir)
            .context("while mangling data_dir output")
            .map_err(Error::Dehydrate)?;
        Ok(())
    }
}

impl Drop for OverlayFs {
    fn drop(&mut self) {
        if let Err(e) = umount(self.mountpoint()).map_err(std::io::Error::from) {
            error!("failed to umount: '{e}'");
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(image_test)]
    #[test]
    fn mount() {}
}
