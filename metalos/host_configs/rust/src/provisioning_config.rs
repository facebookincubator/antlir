/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::packages;
use strum_macros::Display;
use thrift_wrapper::ThriftWrapper;
use url::Url;

#[derive(Debug, Clone, PartialEq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::ProvisioningConfig)]
pub struct ProvisioningConfig {
    #[cfg(facebook)]
    pub deployment_specific: crate::facebook::deployment_specific::DeploymentProvisioningConfig,
    pub identity: HostIdentity,
    pub root_pw_hash: String,
    pub gpt_root_disk: packages::GptRootDisk,
    pub imaging_initrd: packages::ImagingInitrd,
    pub event_backend: EventBackend,
    pub root_disk_config: RootDiskConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::RootDiskConfiguration)]
pub enum RootDiskConfiguration {
    SingleDisk(DiskConfiguration),
    SingleSerial(SingleDiskSerial),
    InvalidMultiDisk(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::SingleDiskSerial)]
pub struct SingleDiskSerial {
    pub serial: String,
    pub config: DiskConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::DiskConfiguration)]
pub struct DiskConfiguration {}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::EventSource)]
pub enum EventSource {
    AssetId(i32),
    Mac(String),
}

#[derive(Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::EventBackend)]
pub struct EventBackend {
    pub base_uri: Url,
    pub source: EventSource,
}

impl std::fmt::Debug for EventBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self { base_uri, source } = self;
        f.debug_struct("EventBackend")
            .field("base_uri", &base_uri.to_string())
            .field("source", source)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::HostIdentity)]
pub struct HostIdentity {
    pub id: String,
    pub hostname: String,
    pub network: Network,
    #[cfg(facebook)]
    pub facebook: crate::facebook::host::HostFacebook,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::Network)]
pub struct Network {
    pub dns: DNS,
    pub interfaces: Vec<NetworkInterface>,
    /// This network interface absolutely must be up for the network to be
    /// considered ready, and is additionally used to setup the default route in
    /// the kernel's routing table
    pub primary_interface: NetworkInterface,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::DNS)]
pub struct DNS {
    pub servers: Vec<String>,
    pub search_domains: Vec<String>,
}

#[derive(Debug, Display, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::NetworkAddressMode)]
pub enum NetworkAddressMode {
    Primary,
    Secondary,
    Deprecated,
}

#[derive(Debug, Display, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::NetworkInterfaceType)]
pub enum NetworkInterfaceType {
    Frontend,
    Backend,
    Oob,
    Mgmt,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::NetworkAddress)]
pub struct NetworkAddress {
    pub addr: String,
    pub prefix_length: i32,
    pub mode: NetworkAddressMode,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::NetworkInterface)]
pub struct NetworkInterface {
    pub mac: String,
    pub addrs: Vec<String>,
    pub name: Option<String>,
    /// This interface is considered necessary and the network will not be
    /// considered up until this interface is configured and up
    pub essential: bool,
    pub structured_addrs: Vec<NetworkAddress>,
    pub interface_type: NetworkInterfaceType,
}
