use anyhow::Context;
use async_trait::async_trait;
use futures::StreamExt;
use slog::{debug, Logger};
use std::path::PathBuf;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use btrfs::{SendstreamExt, Subvolume};
use metalos_host_configs::packages::{Format, Kind, Package};
use thrift_wrapper::ThriftWrapper;

pub mod download;

pub(crate) mod __private {
    pub trait Sealed {}
}

impl<K: Kind> __private::Sealed for Package<K, Uuid> {}

#[async_trait]
pub trait PackageExt: __private::Sealed {
    /// Load the artifacts(s) associated with this package from disk, if they
    /// exist.
    fn on_disk(&self) -> Option<PathBuf>;

    /// Download the artifact(s) associated with this package using some
    /// [download::Downloader] implementation.
    async fn download<D>(&self, log: Logger, dl: D) -> anyhow::Result<PathBuf>
    where
        D: download::Downloader + Send + Sync,
        <D as download::Downloader>::Sendstream: Send;
}

/// Return the path where the artifact(s) for this package should be
/// installed on the local disk.
fn path_on_disk<K: Kind>(package: &Package<K, Uuid>) -> PathBuf {
    metalos_paths::images()
        .join(K::NAME.to_lowercase().replace('_', "-"))
        .join(package.identifier())
}

#[async_trait]
impl<K: Kind> PackageExt for Package<K, Uuid> {
    fn on_disk(&self) -> Option<PathBuf> {
        let dest = path_on_disk(self);
        if dest.exists() { Some(dest) } else { None }
    }

    async fn download<D>(&self, log: Logger, dl: D) -> anyhow::Result<PathBuf>
    where
        D: download::Downloader + Send + Sync,
        <D as download::Downloader>::Sendstream: Send,
    {
        if let Some(artifacts) = self.on_disk() {
            return Ok(artifacts);
        }

        let dest = path_on_disk(self);

        std::fs::create_dir_all(dest.parent().unwrap())
            .with_context(|| format!("while creating parent directory for {}", dest.display()))?;

        let path = match self.format {
            Format::Sendstream => {
                debug!(log, "receiving {:?} into {}", self, dest.display());

                let sendstream = dl
                    .open_sendstream(log, self)
                    .await
                    .with_context(|| format!("while starting sendstream for {:?}", self))?;

                sendstream
                    .receive_into(&dest)
                    .await
                    .context("while receiving")
                    .map(|subvol| subvol.path().to_owned())
            }
            Format::File => {
                let tmp_dest = path_on_disk(self)
                    .parent()
                    .unwrap()
                    .join(format!(".tmp.{}", self.identifier()));
                debug!(log, "downloading {:?} to {}", self, tmp_dest.display());
                let mut dest = tokio::fs::File::create(&tmp_dest)
                    .await
                    .with_context(|| format!("while creating {:?}", tmp_dest.display()))?;
                let mut stream = dl
                    .open_bytes_stream(log, self)
                    .await
                    .with_context(|| format!("while starting http stream for {:?}", self))?;

                while let Some(item) = stream.next().await {
                    dest.write_all(&item?).await?;
                }

                tokio::fs::rename(&tmp_dest, path_on_disk(self))
                    .await
                    .with_context(|| {
                        format!(
                            "while moving {} -> {}",
                            tmp_dest.display(),
                            path_on_disk(self).display()
                        )
                    })?;

                Ok(path_on_disk(self))
            }
        }?;

        let mut subvol = match self.format {
            Format::Sendstream => Some(
                Subvolume::get(&path)
                    .with_context(|| format!("while getting subvol at {}", path.display()))?,
            ),
            _ => None,
        };
        if let Some(ref mut subvol) = subvol {
            subvol
                .set_readonly(false)
                .with_context(|| format!("while marking subvol at {} rw", path.display()))?;
        }
        xattr::set(
            &path,
            "user.metalos.package",
            &fbthrift::simplejson_protocol::serialize(&self.clone().into_thrift()),
        )
        .with_context(|| {
            format!(
                "while writing user.metalos.package xattr on {}",
                path.display()
            )
        })?;
        if let Some(ref mut subvol) = subvol {
            subvol
                .set_readonly(true)
                .with_context(|| format!("while marking subvol at {} ro", path.display()))?;
        }

        Ok(path)
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
