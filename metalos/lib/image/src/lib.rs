use anyhow::Context;
use async_trait::async_trait;
use futures::StreamExt;
use slog::{debug, Logger};
use std::path::PathBuf;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use btrfs::{SendstreamExt, Subvolume};

pub mod download;

use metalos_host_configs::packages::{Initrd, Kernel, PackageId, Rootfs};

pub(crate) mod __private {
    pub trait Sealed {}
}

#[async_trait]
pub trait Package: __private::Sealed {
    type Artifacts;

    /// All of the individual package ids that make up this logical Package
    fn ids(&self) -> Vec<PackageId>;

    /// Load the artifacts(s) associated with this package from disk, if they
    /// exist.
    fn on_disk(&self) -> Option<Self::Artifacts>;

    /// Download the artifact(s) associated with this package from some
    /// [download::Downloader] implementation.
    async fn download<D>(&self, log: Logger, dl: D) -> anyhow::Result<Self::Artifacts>
    where
        D: download::Downloader + Send + Sync,
        <D as download::Downloader>::Sendstream: Send;
}

trait SingleSubvolumePackage: __private::Sealed {
    const KIND: &'static str;
    fn id(&self) -> &PackageId;
    fn path_on_disk(&self) -> PathBuf {
        metalos_paths::images().join(Self::KIND).join(format!(
            "{}:{}",
            self.id().name,
            self.id().uuid
        ))
    }
}

#[async_trait]
impl<T: SingleSubvolumePackage + Sync> Package for T {
    type Artifacts = Subvolume;

    fn ids(&self) -> Vec<PackageId> {
        vec![self.id().clone()]
    }

    fn on_disk(&self) -> Option<Self::Artifacts> {
        let dest = self.path_on_disk();
        Subvolume::get(dest).ok()
    }

    async fn download<D>(&self, log: Logger, dl: D) -> anyhow::Result<Self::Artifacts>
    where
        D: download::Downloader + Send + Sync,
        <D as download::Downloader>::Sendstream: Send,
    {
        if let Some(artifacts) = self.on_disk() {
            return Ok(artifacts);
        }

        let dest = self.path_on_disk();

        std::fs::create_dir_all(dest.parent().unwrap())
            .with_context(|| format!("while creating parent directory for {}", dest.display()))?;

        debug!(log, "receiving {:?} into {}", self.id(), dest.display());

        let sendstream = dl
            .open_sendstream(log, self.id())
            .await
            .with_context(|| format!("while starting sendstream for {:?}", self.id()))?;

        sendstream
            .receive_into(&dest)
            .await
            .context("while receiving")
    }
}

impl __private::Sealed for Rootfs {}

impl SingleSubvolumePackage for Rootfs {
    const KIND: &'static str = "rootfs";
    fn id(&self) -> &PackageId {
        &self.id
    }
}

impl __private::Sealed for Kernel {}

impl SingleSubvolumePackage for Kernel {
    const KIND: &'static str = "kernel";
    fn id(&self) -> &PackageId {
        &self.kernel
    }
}

impl __private::Sealed for Initrd {}

trait InitrdExt {
    fn path_on_disk(&self) -> PathBuf;
}

impl InitrdExt for Initrd {
    fn path_on_disk(&self) -> PathBuf {
        metalos_paths::images()
            .join("initrd")
            .join(format!("{}:{}", self.id.name, self.id.uuid))
    }
}

#[async_trait]
impl Package for Initrd {
    type Artifacts = PathBuf;

    fn ids(&self) -> Vec<PackageId> {
        vec![self.id.clone()]
    }

    fn on_disk(&self) -> Option<Self::Artifacts> {
        let dest = self.path_on_disk();
        if dest.exists() { Some(dest) } else { None }
    }

    async fn download<D>(&self, log: Logger, dl: D) -> anyhow::Result<Self::Artifacts>
    where
        D: download::Downloader + Send + Sync,
        <D as download::Downloader>::Sendstream: Send,
    {
        if let Some(artifacts) = self.on_disk() {
            return Ok(artifacts);
        }

        let mut dest = tokio::fs::File::create(self.path_on_disk()).await?;
        let mut stream = dl
            .open_bytes_stream(log, &self.id)
            .await
            .with_context(|| format!("while starting http stream for {:?}", self.id))?;

        while let Some(item) = stream.next().await {
            dest.write_all(&item?).await?;
        }

        Ok(self.path_on_disk())
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("download error {0:?}")]
    Download(#[from] download::Error),
    #[error("btrfs error {0:?}")]
    BtrfsError(#[from] btrfs::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
