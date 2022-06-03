/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::boot_config;
use crate::provisioning_config;
use crate::runtime_config;
use thrift_wrapper::ThriftWrapper;

/// Main entrypoint for a MetalOS host.
#[derive(Debug, Clone, PartialEq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::host::HostConfig)]
pub struct HostConfig {
    pub provisioning_config: provisioning_config::ProvisioningConfig,
    pub boot_config: boot_config::BootConfig,
    pub runtime_config: runtime_config::RuntimeConfig,
}
