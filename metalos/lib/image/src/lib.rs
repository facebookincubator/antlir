use anyhow::Context;
use derive_more::{Deref, Display, From, FromStr};
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Type parameters for [Image].
pub mod kinds;
pub use kinds::{ConfigImage, KernelImage, Kind, RootfsImage, ServiceImage, WdsImage};
pub mod download;

#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

use package_manifest::types::Image as ThriftImage;

pub(crate) mod __private {
    pub trait Sealed {}
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("unknown image kind ({0})")]
    UnknownKind(i32),
    #[error("wrong image kind (wanted {wanted}, got {actual})")]
    WrongKind { wanted: Kind, actual: Kind },
    #[error("download error {0:?}")]
    Download(#[from] download::Error),
    #[error("btrfs error {0:?}")]
    BtrfsError(#[from] btrfs::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Image package name. Together with [ImageID] it uniquely (and globally)
/// references an image.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, From, Display, Deref, FromStr
)]
#[from(forward)]
#[display(forward)]
#[deref(forward)]
#[repr(transparent)]
pub struct ImageName(String);

/// Image package version identifier. Together with [ImageName] it uniquely
/// (and globally) references an image.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, From, Display, Deref, FromStr
)]
#[from(forward)]
#[display(forward)]
#[deref(forward)]
#[repr(transparent)]
pub struct ImageID(String);

/// Safer version of [AnyImage] that can constrain images to a specific kind at
/// compile time, instead of requiring runtime checks anywhere that image-kind
/// specialization is required.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Image<K: kinds::ConstKind>(AnyImage, PhantomData<K>);

impl<K: kinds::ConstKind> Deref for Image<K> {
    type Target = AnyImage;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K: kinds::ConstKind> TryFrom<AnyImage> for Image<K> {
    type Error = Error;

    fn try_from(i: AnyImage) -> Result<Self> {
        if i.kind == K::KIND {
            Ok(Self(i, PhantomData))
        } else {
            Err(Error::WrongKind {
                wanted: K::KIND,
                actual: i.kind,
            })
        }
    }
}

impl<K: kinds::ConstKind> TryFrom<ThriftImage> for Image<K> {
    type Error = Error;

    fn try_from(i: ThriftImage) -> Result<Self> {
        i.try_into().and_then(|i: AnyImage| i.try_into())
    }
}

impl<K: kinds::ConstKind> TryFrom<&str> for Image<K> {
    type Error = anyhow::Error;

    fn try_from(package_str: &str) -> anyhow::Result<Self> {
        let (name, id) = package_str
            .split_once(':')
            .context("expected ':' separator")?;

        Ok(Self(
            AnyImage {
                name: ImageName(name.to_string()),
                id: ImageID(id.to_string()),
                kind: K::KIND,
                override_uri: None,
            },
            PhantomData,
        ))
    }
}

impl<K: kinds::ConstKind> From<Image<K>> for ThriftImage {
    fn from(i: Image<K>) -> Self {
        i.0.into()
    }
}

/// Type-erased image of an arbitrary [Kind].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnyImage {
    name: ImageName,
    id: ImageID,
    kind: Kind,
    override_uri: Option<String>,
}

impl AnyImage {
    pub fn name(&self) -> &ImageName {
        &self.name
    }

    pub fn id(&self) -> &ImageID {
        &self.id
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn path_on_disk(&self, basedir: &Path) -> PathBuf {
        basedir.join(format!("{}/{}:{}", self.kind.as_str(), self.name, self.id))
    }
}

impl TryFrom<ThriftImage> for AnyImage {
    type Error = Error;

    fn try_from(i: ThriftImage) -> Result<Self> {
        Ok(Self {
            name: i.name.into(),
            id: i.id.into(),
            kind: i.kind.try_into()?,
            override_uri: i.override_uri,
        })
    }
}

impl<K: kinds::ConstKind> From<Image<K>> for AnyImage {
    fn from(i: Image<K>) -> Self {
        i.0
    }
}

impl From<AnyImage> for ThriftImage {
    fn from(i: AnyImage) -> Self {
        Self {
            name: i.name.0,
            id: i.id.0,
            kind: i.kind.into(),
            override_uri: i.override_uri,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use package_manifest::types::{Image as ThriftImage, Kind as ThriftKind};

    #[test]
    fn conversions() -> anyhow::Result<()> {
        let t = ThriftImage {
            name: "hello".into(),
            id: "world".into(),
            kind: ThriftKind::KERNEL,
            override_uri: None,
        };
        let ai: AnyImage = t.try_into()?;
        assert_eq!(
            AnyImage {
                name: "hello".into(),
                id: "world".into(),
                kind: Kind::Kernel,
                override_uri: None,
            },
            ai,
        );
        assert!(KernelImage::try_from(ai.clone()).is_ok());
        assert!(RootfsImage::try_from(ai).is_err());
        Ok(())
    }
}
