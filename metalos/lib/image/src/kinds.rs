/// Type parameters for [Image].
use crate::{Error, Image, Result, __private::Sealed};
use derive_more::Display;

use package_manifest::types::Kind as ThriftKind;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Display)]
#[repr(u8)]
pub enum Kind {
    Rootfs,
    Config,
    Kernel,
    Service,
    Wds,
    GptRootdisk,
}

impl Kind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Rootfs => "rootfs",
            Self::Config => "config",
            Self::Kernel => "kernel",
            Self::Service => "service",
            Self::Wds => "wds",
            Self::GptRootdisk => "gpt-rootdisk",
        }
    }
}

impl TryFrom<ThriftKind> for Kind {
    type Error = Error;

    fn try_from(k: ThriftKind) -> Result<Self> {
        match k {
            ThriftKind::ROOTFS => Ok(Self::Rootfs),
            ThriftKind::CONFIG => Ok(Self::Config),
            ThriftKind::KERNEL => Ok(Self::Kernel),
            ThriftKind::SERVICE => Ok(Self::Service),
            ThriftKind::WDS => Ok(Self::Wds),
            ThriftKind::GPT_ROOTDISK => Ok(Self::GptRootdisk),
            _ => Err(Error::UnknownKind(k.0)),
        }
    }
}

impl From<Kind> for ThriftKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Rootfs => ThriftKind::ROOTFS,
            Kind::Config => ThriftKind::CONFIG,
            Kind::Kernel => ThriftKind::KERNEL,
            Kind::Service => ThriftKind::SERVICE,
            Kind::Wds => ThriftKind::WDS,
            Kind::GptRootdisk => ThriftKind::GPT_ROOTDISK,
        }
    }
}

pub trait ConstKind: Sealed + std::fmt::Debug {
    const KIND: Kind;
}

/// See [Kind::RootFs].
#[derive(Debug, Clone, Copy)]
pub struct Rootfs();
impl ConstKind for Rootfs {
    const KIND: Kind = Kind::Rootfs;
}
impl Sealed for Rootfs {}
pub type RootfsImage = Image<Rootfs>;

/// See [Kind::Config].
#[derive(Debug, Clone, Copy)]
pub struct Config();
impl ConstKind for Config {
    const KIND: Kind = Kind::Config;
}
impl Sealed for Config {}
pub type ConfigImage = Image<Config>;

/// See [Kind::Kernel].
#[derive(Debug, Clone, Copy)]
pub struct Kernel();
impl ConstKind for Kernel {
    const KIND: Kind = Kind::Kernel;
}
impl Sealed for Kernel {}
pub type KernelImage = Image<Kernel>;

/// See [Kind::Service].
#[derive(Debug, Clone, Copy)]
pub struct Service();
impl ConstKind for Service {
    const KIND: Kind = Kind::Service;
}
impl Sealed for Service {}
pub type ServiceImage = Image<Service>;

/// See [Kind::Wds].
#[derive(Debug, Clone, Copy)]
pub struct Wds();
impl ConstKind for Wds {
    const KIND: Kind = Kind::Wds;
}
impl Sealed for Wds {}
pub type WdsImage = Image<Wds>;

/// See [Kind::GptRootdisk].
#[derive(Debug, Clone, Copy)]
pub struct GptRootdisk();
impl ConstKind for GptRootdisk {
    const KIND: Kind = Kind::GptRootdisk;
}
impl Sealed for GptRootdisk {}
pub type GptRootdiskImage = Image<GptRootdisk>;
