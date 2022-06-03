/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::packages;
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
    #[deprecated = "use event_backend struct instead"]
    pub event_backend_base_uri: String,
    pub event_backend: EventBackend,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::EventSource)]
pub enum EventSource {
    AssetId(i32),
    Mac(String),
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::EventBackend)]
pub struct EventBackend {
    pub base_uri: Url,
    pub source: EventSource,
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

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::provisioning_config::NetworkInterface)]
pub struct NetworkInterface {
    pub mac: String,
    pub addrs: Vec<String>,
    pub name: Option<String>,
    /// This interface is considered necessary and the network will not be
    /// considered up until this interface is configured and up
    pub essential: bool,
}
