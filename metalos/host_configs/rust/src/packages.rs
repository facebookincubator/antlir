/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::marker::PhantomData;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use metalos_thrift_host_configs::packages::Kind as ThriftKind;
use strum_macros::Display;
use thrift_wrapper::Error;
use thrift_wrapper::FieldContext;
use thrift_wrapper::Result;
use thrift_wrapper::ThriftWrapper;
use url::Url;
use uuid::Uuid;

#[derive(
    Debug,
    Display,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ThriftWrapper
)]
#[thrift(metalos_thrift_host_configs::packages::Format)]
pub enum Format {
    Sendstream,
    File,
}

pub(crate) mod __private {
    pub trait Sealed {}
}

pub trait Kind:
    Debug + Copy + Clone + PartialEq + Eq + PartialOrd + Ord + Sync + Send + __private::Sealed
{
    const NAME: &'static str;
    const THRIFT: ThriftKind;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageTag(String);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package<K: Kind, Id> {
    pub name: String,
    pub id: Id,
    pub override_uri: Option<Url>,
    pub format: Format,
    kind: PhantomData<K>,
}

impl<K: Kind, Id: Debug> std::fmt::Debug for Package<K, Id> {
    #[deny(unused_variables)]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Package {
            name,
            id,
            override_uri,
            format,
            kind: _,
        } = self;
        let mut s = f.debug_struct("Package");
        s.field("name", name)
            .field("id", id)
            .field("format", format);
        if let Some(u) = override_uri {
            s.field("override_uri", &u.to_string());
        }
        s.finish()
    }
}

impl<K> Package<K, Uuid>
where
    K: Kind,
{
    pub fn new(name: String, uuid: Uuid, override_uri: Option<Url>, format: Format) -> Self {
        Self {
            name,
            id: uuid,
            override_uri,
            format,
            kind: PhantomData,
        }
    }

    pub fn identifier(&self) -> String {
        format!("{}:{}", self.name, self.id.to_simple())
    }

    /// Return the path where the artifact(s) for this package should be
    /// installed on the local disk.
    pub fn path(&self) -> PathBuf {
        generic::Package::from(self.clone()).path()
    }
}

macro_rules! package_thrift_wrapper {
    ($idt:ty, $id_from_thrift:expr, $id_to_thrift:expr) => {
        impl<K> ThriftWrapper for Package<K, $idt>
        where
            K: Kind,
        {
            type Thrift = metalos_thrift_host_configs::packages::Package;
            fn from_thrift(t: metalos_thrift_host_configs::packages::Package) -> Result<Self> {
                let metalos_thrift_host_configs::packages::Package {
                    name,
                    id,
                    override_uri,
                    kind,
                    format,
                } = t;
                let override_uri = match override_uri {
                    Some(uri) => {
                        let uri = Url::from_thrift(uri).in_field("override_uri")?;
                        Some(uri)
                    }
                    None => None,
                };
                if kind != K::THRIFT {
                    return Err(Error::Nested {
                        field: "kind".into(),
                        error: Box::new(Error::PackageKind {
                            expected: K::NAME,
                            actual: kind.to_string(),
                        }),
                    });
                }
                let id = $id_from_thrift(id).in_field("id")?;
                Ok(Self {
                    name,
                    id,
                    override_uri,
                    format: format.try_into().in_field("format")?,
                    kind: PhantomData,
                })
            }

            fn into_thrift(self) -> metalos_thrift_host_configs::packages::Package {
                metalos_thrift_host_configs::packages::Package {
                    name: self.name,
                    id: $id_to_thrift(self.id),
                    override_uri: self.override_uri.map(|u| u.to_string()),
                    format: self.format.into(),
                    kind: K::THRIFT,
                }
            }
        }

        impl<K> TryFrom<metalos_thrift_host_configs::packages::Package> for Package<K, $idt>
        where
            K: Kind,
        {
            type Error = Error;

            fn try_from(t: metalos_thrift_host_configs::packages::Package) -> Result<Self> {
                Self::from_thrift(t)
            }
        }
    };
}

package_thrift_wrapper!(
    Uuid,
    |id| match id {
        metalos_thrift_host_configs::packages::PackageId::uuid(u) => {
            Uuid::from_thrift(u)
        }
        other => Err(Error::from(anyhow!("{:?} is not a uuid", other))),
    },
    |id: Uuid| metalos_thrift_host_configs::packages::PackageId::uuid(id.to_simple().to_string())
);

package_thrift_wrapper!(
    PackageTag,
    |id| match id {
        metalos_thrift_host_configs::packages::PackageId::tag(t) => Ok(PackageTag(t)),
        // the tagged variant is just an arbitrary string, so it can be the
        // uuid as well - this is useful for interaction with
        // ConfigProviders which may return a mixture of uuids and tags
        metalos_thrift_host_configs::packages::PackageId::uuid(u) => Ok(PackageTag(u)),
        other => Err(Error::from(anyhow!("{:?} is not a tag", other))),
    },
    |id: PackageTag| metalos_thrift_host_configs::packages::PackageId::tag(id.0)
);

impl<K> From<Package<K, Uuid>> for metalos_thrift_host_configs::packages::Package
where
    K: Kind,
{
    fn from(pkg: Package<K, Uuid>) -> Self {
        pkg.into_thrift()
    }
}

impl<K> Package<K, PackageTag>
where
    K: Kind,
{
    pub fn new(name: String, tag: PackageTag, override_uri: Option<Url>, format: Format) -> Self {
        Self {
            name,
            id: tag,
            override_uri,
            format,
            kind: PhantomData,
        }
    }

    /// Convert this to the more friendly uuid-versioned Package type after
    /// externally resolving the tag to a uuid.
    pub fn with_uuid(self, uuid: Uuid) -> Package<K, Uuid> {
        Package {
            name: self.name,
            id: uuid,
            override_uri: self.override_uri,
            format: self.format,
            kind: PhantomData,
        }
    }
}

macro_rules! package_kind_param {
    ($i:ident, $k:ident, $tk:ident) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $k;

        impl __private::Sealed for $k {}

        impl Kind for $k {
            const NAME: &'static str = stringify!($i);
            const THRIFT: ThriftKind = metalos_thrift_host_configs::packages::Kind::$tk;
        }

        /// The type for each Kind is always equal with any other instances,
        /// since there is only one value
        impl std::cmp::PartialOrd for $k {
            fn partial_cmp(&self, _: &Self) -> Option<std::cmp::Ordering> {
                Some(std::cmp::Ordering::Equal)
            }
        }

        impl std::cmp::Ord for $k {
            fn cmp(&self, _: &Self) -> std::cmp::Ordering {
                std::cmp::Ordering::Equal
            }
        }

        pub type $i = Package<$k, Uuid>;
    };
}

package_kind_param!(Rootfs, RootfsKind, ROOTFS);
package_kind_param!(Kernel, KernelKind, KERNEL);
package_kind_param!(Initrd, InitrdKind, INITRD);
package_kind_param!(ImagingInitrd, ImagingInitrdKind, IMAGING_INITRD);
package_kind_param!(Service, ServiceKind, SERVICE);
package_kind_param!(
    ServiceConfigGenerator,
    ServiceConfigGeneratorKind,
    SERVICE_CONFIG_GENERATOR
);
package_kind_param!(GptRootDisk, GptRootDiskKind, GPT_ROOT_DISK);
package_kind_param!(Bootloader, BootloaderKind, BOOTLOADER);

// Some package kinds have some extra data that we can use, so expose it nicely
// via the `Package<K>` structs

impl<K: Kind> Package<K, Uuid> {
    /// Get absolute path to a file given a relative path. Checks to ensure that
    /// the file exists, otherwise will return None
    pub(crate) fn file_in_image(&self, relpath: impl AsRef<Path>) -> Option<PathBuf> {
        let path = self.path().join(relpath.as_ref());
        match path.exists() {
            true => Some(path),
            false => None,
        }
    }
}

#[derive(Debug, Display, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::packages::InstallationStatus)]
pub enum InstallationStatus {
    Success,
    FailedToDownload,
    FailedToInstall,
    PackageNotFound,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::packages::PackageStatus)]
pub struct PackageStatus {
    pub pkg: generic::Package,
    pub installation_status: InstallationStatus,
    pub error: Option<String>,
}

/// Generic versions of the types above, useful for cases where code wants to
/// (less safely) operate on a collection of heterogenous package kinds.
pub mod generic {
    use std::path::PathBuf;

    use fbthrift::simplejson_protocol::serialize;
    use strum_macros::Display;
    use thrift_wrapper::ThriftWrapper;
    use url::Url;
    use uuid::Uuid;

    use super::Format;

    #[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper, Display)]
    #[thrift(metalos_thrift_host_configs::packages::Kind)]
    pub enum Kind {
        Rootfs,
        Kernel,
        Initrd,
        ImagingInitrd,
        Service,
        ServiceConfigGenerator,
        GptRootDisk,
        Bootloader,
    }

    #[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
    #[thrift(metalos_thrift_host_configs::packages::PackageId)]
    pub enum PackageId {
        Tag(String),
        Uuid(Uuid),
    }

    // A collection of Packages, equipped with human-friendly display formatting.
    #[derive(Clone, Debug, PartialEq, Eq, ThriftWrapper)]
    #[thrift(metalos_thrift_host_configs::packages::Packages)]
    pub struct Packages {
        pub packages: Vec<Package>,
    }

    impl std::fmt::Display for Packages {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            let json = serialize(self);
            match std::str::from_utf8(&json) {
                Ok(utf) => write!(f, "{utf}"),
                Err(_) => Err(std::fmt::Error {}),
            }
        }
    }

    /// Generic version of a Package, without any static type information about
    /// the nature of the package.
    #[derive(Clone, PartialEq, Eq, ThriftWrapper)]
    #[thrift(metalos_thrift_host_configs::packages::Package)]
    pub struct Package {
        pub name: String,
        pub id: PackageId,
        pub override_uri: Option<Url>,
        pub format: Format,
        pub kind: Kind,
    }

    impl Package {
        pub fn identifier(&self) -> String {
            format!(
                "{}:{}",
                self.name,
                match &self.id {
                    PackageId::Uuid(u) => u.to_simple().to_string(),
                    PackageId::Tag(t) => t.clone(),
                }
            )
        }

        /// Return the path where the artifact(s) for this package should be
        /// installed on the local disk.
        pub fn path(&self) -> PathBuf {
            metalos_paths::images::base()
                .join(self.kind.to_string().to_lowercase().replace('_', "-"))
                .join(self.identifier())
        }
    }

    impl std::fmt::Debug for Package {
        #[deny(unused_variables)]
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            let Package {
                name,
                id,
                override_uri,
                format,
                kind: _,
            } = self;
            let mut s = f.debug_struct("Package");
            s.field("name", name)
                .field("id", id)
                .field("format", format);
            if let Some(u) = override_uri {
                s.field("override_uri", &u.to_string());
            }
            s.finish()
        }
    }

    /// Only allow conversion from a UUID-identified Package to discourage
    /// on-host usage of tags (the HostConfig will already only be deserialized
    /// if it only contains uuids, but that doesn't stop people from
    /// constructing less-safe structs locally).
    impl<K: super::Kind> From<super::Package<K, Uuid>> for Package {
        fn from(p: super::Package<K, Uuid>) -> Self {
            Self {
                name: p.name,
                id: PackageId::Uuid(p.id),
                override_uri: p.override_uri,
                format: p.format,
                kind: K::THRIFT
                    .try_into()
                    .expect("compiler statically ensures all variants are covered"),
            }
        }
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;

    use super::*;

    #[test]
    fn format_enum() -> Result<()> {
        assert_eq!(
            Format::Sendstream,
            metalos_thrift_host_configs::packages::Format::SENDSTREAM.try_into()?,
        );
        assert_eq!(
            Format::File,
            metalos_thrift_host_configs::packages::Format::FILE.try_into()?,
        );
        match Format::try_from(metalos_thrift_host_configs::packages::Format(100000)) {
            Err(Error::Enum(s)) => assert_eq!("Format::100000", s),
            _ => panic!("expected Error::Enum"),
        };

        Ok(())
    }

    #[test]
    fn package_conversions() -> Result<()> {
        let id = Uuid::new_v4();
        assert_eq!(
            Rootfs {
                name: "metalos.rootfs".into(),
                id,
                override_uri: None,
                format: Format::Sendstream,
                kind: PhantomData,
            },
            metalos_thrift_host_configs::packages::Package {
                name: "metalos.rootfs".into(),
                id: metalos_thrift_host_configs::packages::PackageId::uuid(
                    id.to_simple().to_string()
                ),
                override_uri: None,
                kind: ThriftKind::ROOTFS,
                format: metalos_thrift_host_configs::packages::Format::SENDSTREAM,
            }
            .try_into()?
        );
        assert_eq!(
            metalos_thrift_host_configs::packages::Package {
                name: "metalos.rootfs".into(),
                id: metalos_thrift_host_configs::packages::PackageId::uuid(
                    id.to_simple().to_string()
                ),
                override_uri: None,
                kind: ThriftKind::ROOTFS,
                format: metalos_thrift_host_configs::packages::Format::SENDSTREAM,
            },
            Rootfs {
                name: "metalos.rootfs".into(),
                id,
                override_uri: None,
                format: Format::Sendstream,
                kind: PhantomData,
            }
            .into()
        );
        Ok(())
    }
}
