/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use futures::future::try_join_all;
use futures::Stream;
use futures::StreamExt;
use slog::debug;
use slog::Logger;
use tempfile::NamedTempFile;
use thiserror::Error;

use metalos_host_configs::packages::Format;
use metalos_host_configs::packages::{self};

mod https;
pub use https::HttpsDownloader;

use btrfs::sendstream::Zstd;
use btrfs::Sendstream;
use btrfs::SendstreamExt;
use btrfs::Subvolume;

#[derive(Error, Debug)]
pub enum Error {
    #[error("package '{package}' was not found", package = .0.identifier())]
    NotFound(packages::generic::Package),
    #[error(
        "failed to download '{package}': {error}",
        package = package.identifier(),
    )]
    Download {
        package: packages::generic::Package,
        error: anyhow::Error,
    },
    #[error(
        "failed to install package '{package}': {error}",
        package = package.identifier(),
    )]
    Install {
        package: packages::generic::Package,
        error: anyhow::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[async_trait]
pub trait PackageDownloader {
    type BytesStream: Stream<Item = std::io::Result<Bytes>> + Unpin + Send;

    /// Open a bytes stream from the underlying image source.
    async fn open_bytes_stream(
        &self,
        log: Logger,
        package: &packages::generic::Package,
    ) -> Result<Self::BytesStream>;
}

pub trait PackageExt: Clone + Into<packages::generic::Package> {
    type Artifact;

    /// Load the artifacts(s) associated with this package from disk, if they
    /// exist.
    fn on_disk(&self) -> Option<Self::Artifact>;
}

macro_rules! subvol_package {
    ($p:ty) => {
        impl PackageExt for $p {
            type Artifact = Subvolume;

            fn on_disk(&self) -> Option<Self::Artifact> {
                Subvolume::get(self.path()).ok()
            }
        }
    };
}

macro_rules! file_package {
    ($p:ty) => {
        impl PackageExt for $p {
            type Artifact = PathBuf;

            fn on_disk(&self) -> Option<Self::Artifact> {
                let dest = self.path();
                if dest.exists() { Some(dest) } else { None }
            }
        }
    };
}

subvol_package!(packages::Rootfs);
subvol_package!(packages::Kernel);
subvol_package!(packages::Service);
file_package!(packages::Initrd);
file_package!(packages::ImagingInitrd);
file_package!(packages::GptRootDisk);
file_package!(packages::Bootloader);

/// Make sure that a given package is on disk, downloading it if it is not
/// already locally available.
pub async fn ensure_package_on_disk<D, P, A>(log: Logger, dl: D, pkgext: P) -> Result<A>
where
    D: PackageDownloader,
    P: PackageExt<Artifact = A>,
{
    if let Some(artifacts) = pkgext.on_disk() {
        return Ok(artifacts);
    }

    let pkg: packages::generic::Package = pkgext.clone().into();

    ensure_package_on_disk_ignoring_artifacts(log, dl, &pkg).await?;

    pkgext
        .on_disk()
        .context("package supposedly downloaded but was not on disk")
        .map_err(|error| Error::Install {
            error,
            package: pkg,
        })
}

/// Make sure the provided packages are on disk, downloading if not already available.
pub async fn ensure_packages_on_disk_ignoring_artifacts<D>(
    log: Logger,
    dl: D,
    pkgs: &[packages::generic::Package],
) -> Result<()>
where
    D: PackageDownloader + Clone,
{
    try_join_all(pkgs.iter().map(|package| {
        let log = log.clone();
        let downloader = dl.clone();
        async move { ensure_package_on_disk_ignoring_artifacts(log, downloader, package).await }
    }))
    .await
    .map(|_| ())
}

/// Make sure that a single package is on disk, downloading it if it is not
/// already locally available. Unlike [ensure_package_on_disk], this function
/// does not statically know the type of the package artifact on disk, so it
/// does not return the downloaded artifact.
async fn ensure_package_on_disk_ignoring_artifacts<D>(
    log: Logger,
    dl: D,
    pkg: &packages::generic::Package,
) -> Result<()>
where
    D: PackageDownloader,
{
    let dest = pkg.path();

    if dest.exists() {
        return Ok(());
    }

    let map_install_err = |error: anyhow::Error| Error::Install {
        error,
        package: pkg.clone(),
    };

    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("while creating parent directory for {}", dest.display()))
        .map_err(|error| Error::Install {
            error,
            package: pkg.clone(),
        })?;

    let mut stream = dl.open_bytes_stream(log.clone(), pkg).await?;

    match pkg.format {
        Format::Sendstream => {
            debug!(log, "receiving {:?} into {}", pkg, dest.display());
            let sendstream = Sendstream::<Zstd, _>::new(stream);

            let mut subvol = sendstream
                .receive_into(&dest)
                .await
                .map_err(anyhow::Error::msg)
                .map_err(map_install_err)?;

            // set the subvolume to readwrite so that we can write xattrs on it
            subvol
                .set_readonly(false)
                .context("while setting subvolume rw")
                .map_err(map_install_err)?;
        }
        Format::File => {
            let mut tmp_dest = NamedTempFile::new_in(dest.parent().unwrap())
                .with_context(|| {
                    format!(
                        "while creating temporary file in {}",
                        dest.parent().unwrap().display()
                    )
                })
                .map_err(map_install_err)?;
            debug!(
                log,
                "downloading {:?} to {}",
                pkg,
                tmp_dest.path().display()
            );

            while let Some(item) = stream.next().await {
                tmp_dest
                    .write_all(
                        &item
                            .context("while reading chunk from downloader")
                            .map_err(|error| Error::Download {
                                error,
                                package: pkg.clone(),
                            })?,
                    )
                    .with_context(|| {
                        format!("while writing chunk to {}", tmp_dest.path().display())
                    })
                    .map_err(map_install_err)?;
            }

            let tmp_dest_path = tmp_dest.path().to_path_buf();

            tmp_dest
                .persist(&dest)
                .with_context(|| {
                    format!(
                        "while moving {} -> {}",
                        tmp_dest_path.display(),
                        dest.display()
                    )
                })
                .map_err(map_install_err)?;
        }
    };

    xattr::set(
        &dest,
        "user.metalos.package",
        &fbthrift::simplejson_protocol::serialize(&pkg),
    )
    .with_context(|| {
        format!(
            "while writing user.metalos.package xattr on {}",
            dest.display()
        )
    })
    .map_err(map_install_err)?;

    match pkg.format {
        Format::Sendstream => {
            Subvolume::get(&dest)
                .context("while getting subvol")
                .map_err(map_install_err)?
                .set_readonly(true)
                .context("while setting subvol ro")
                .map_err(map_install_err)?;
        }
        Format::File => {
            let perm = Permissions::from_mode(0o444);
            tokio::fs::set_permissions(&dest, perm)
                .await
                .with_context(|| format!("while setting {} readonly", dest.display()))
                .map_err(map_install_err)?;
        }
    }

    Ok(())
}
