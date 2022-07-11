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

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use fbthrift::simplejson_protocol::deserialize;
use futures::future::try_join_all;
use futures::Stream;
use futures::StreamExt;
use futures::TryStream;
use futures::TryStreamExt;
use slog::debug;
use slog::Logger;
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::task::spawn_blocking;
use tokio_stream::wrappers::ReadDirStream;

use metalos_host_configs::packages::Format;
use metalos_host_configs::packages::{self};

mod https;
pub use https::HttpsDownloader;

use btrfs::sendstream::Zstd;
use btrfs::Sendstream;
use btrfs::SendstreamExt;
use btrfs::Subvolume;

const XATTR_KEY: &str = "user.metalos.package";

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
    #[error("failed while reading packages from disk: {error}")]
    Read { error: anyhow::Error },
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

    fs::create_dir_all(dest.parent().unwrap())
        .await
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
            // We use a tempfile to ensure we generate an unused file path, and for its destructor
            // cleanup. However, to allow for async-friendly IO, we don't write through the
            // NamedTempFile handle. Instead, we open a second file File, which shares the same
            // filesystem handle, and use it to construct an async-friendly tokio::fs::File. We use
            // that struct for async writing.
            let (tmpfile, tmpfile_path) = NamedTempFile::new_in(dest.parent().unwrap())
                .with_context(|| {
                    format!(
                        "while creating temporary file in {}",
                        dest.parent().unwrap().display()
                    )
                })
                .map_err(map_install_err)?
                .into_parts();
            debug!(log, "downloading {:?} to {}", pkg, tmpfile_path.display());
            let mut sink = fs::File::from_std(tmpfile);

            while let Some(item) = stream.next().await {
                sink.write_all(
                    &item
                        .context("while reading chunk from downloader")
                        .map_err(|error| Error::Download {
                            error,
                            package: pkg.clone(),
                        })?,
                )
                .await
                .with_context(|| format!("while writing chunk to {}", tmpfile_path.display()))
                .map_err(map_install_err)?;
            }

            fs::rename(&tmpfile_path, &dest)
                .await
                .with_context(|| {
                    format!(
                        "while moving {} -> {}",
                        tmpfile_path.display(),
                        dest.display()
                    )
                })
                .map_err(map_install_err)?;
        }
    };

    xattr::set(
        &dest,
        XATTR_KEY,
        &fbthrift::simplejson_protocol::serialize(&pkg),
    )
    .with_context(|| format!("while writing {XATTR_KEY} xattr on {}", dest.display()))
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
            fs::set_permissions(&dest, perm)
                .await
                .with_context(|| format!("while setting {} readonly", dest.display()))
                .map_err(map_install_err)?;
        }
    }

    Ok(())
}

/// Inventory the packages storaged on disk. Returns discovered packages as a TryStream, as we may
/// discover errors along the way.
pub async fn staged_packages()
-> Result<impl TryStream<Ok = packages::generic::Package, Error = Error>> {
    let root = metalos_paths::images();
    let subdirs = fs::read_dir(metalos_paths::images())
        .await
        .context(format!("while getting children paths on {root:?}"))
        .map_err(|error| Error::Read { error })?;
    Ok(ReadDirStream::new(subdirs)
        .map(|maybe_dentry| {
            maybe_dentry.context(format!("while reading child of {:?}", root.to_owned()))
        })
        .try_filter_map(|dentry| async move {
            let path = dentry.path();
            if dentry.path().is_dir() {
                let subdir = fs::read_dir(&path)
                    .await
                    .context(format!("while reading children paths on {path:?}"))?;
                Ok(Some(
                    ReadDirStream::new(subdir)
                        .map(move |image| image.context(format!("while reading child of {path:?}")))
                        .try_filter_map(|image| async move {
                            let img_path = image.path();
                            let _res: packages::generic::Package = spawn_blocking(move || {
                                // xattr is not async, so we offload it to an executor
                                match xattr::get(&img_path, XATTR_KEY)? {
                                    Some(x) => deserialize(x),
                                    None => Err(anyhow!(format!("Non-image at {img_path:?}"))),
                                }
                            })
                            .await
                            .context(format!("while executing xattr on {:?}", image.path()))?
                            .context(format!("while reading xattr on {:?}", image.path()))?;
                            Ok(Some(_res))
                        }),
                ))
            } else {
                // Skip non-directories under image root
                Ok(None)
            }
        })
        .try_flatten()
        .map_err(|error: anyhow::Error| Error::Read { error }))
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use super::*;
    use anyhow::Result;
    use futures::stream::empty;
    use metalos_macros::containertest;
    use slog::o;
    use slog::Logger;

    /// Use a BlankDownloader as a stub that installs packages (of size zero bytes) without
    /// accessing the network. Note that this only properly simulates packages of Format::File,
    /// because an empty byte string does not make a valid btrfs sendstream.
    struct BlankDownloader {}

    impl BlankDownloader {
        fn new() -> Self {
            Self {}
        }
    }

    #[async_trait]
    impl PackageDownloader for &BlankDownloader {
        type BytesStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

        async fn open_bytes_stream(
            &self,
            _log: Logger,
            _package: &packages::generic::Package,
        ) -> super::Result<Self::BytesStream> {
            Ok(Box::pin(empty()))
        }
    }

    /// Ensure collection of staged packages handles no results gracefully. Here, we assume the
    /// container's layer has no images already staged on the filesystem.
    #[containertest]
    async fn inventory_empty() -> Result<()> {
        let staged: Vec<_> = staged_packages().await?.try_collect().await?;
        assert!(staged.is_empty(), "expected to find no staged packages");
        Ok(())
    }

    /// Stage two packages (using a network-less downloader stub) on disk inside the container and
    /// then prove we can inventory them.
    #[containertest]
    async fn inventory_with_packages() -> Result<()> {
        let log = Logger::root(slog_glog_fmt::default_drain(), o!());
        let downloader = BlankDownloader::new();

        // Put some images down on disk
        ensure_packages_on_disk_ignoring_artifacts(
            log,
            &downloader,
            &Vec::from([
                packages::generic::Package {
                    name: String::from("Foo"),
                    id: packages::generic::PackageId::Tag(String::from("LATEST")),
                    format: Format::File,
                    override_uri: None,
                    kind: packages::generic::Kind::Rootfs,
                },
                packages::generic::Package {
                    name: String::from("Bar"),
                    id: packages::generic::PackageId::Tag(String::from("contbuild")),
                    format: Format::File,
                    override_uri: None,
                    kind: packages::generic::Kind::Initrd,
                },
            ]),
        )
        .await?;

        // Read them
        let staged: Vec<_> = staged_packages().await?.try_collect().await?;
        assert_eq!(2, staged.len(), "expected to find two staged packages");
        Ok(())
    }
}
