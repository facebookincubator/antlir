/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::packages;
use thrift_wrapper::ThriftWrapper;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::boot_config::BootConfig)]
pub struct BootConfig {
    #[cfg(facebook)]
    deployment_specific: crate::facebook::deployment_specific::DeploymentBootConfig,
    pub rootfs: packages::Rootfs,
    pub kernel: Kernel,
    pub initrd: packages::Initrd,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::boot_config::Kernel)]
pub struct Kernel {
    pub pkg: packages::Kernel,
    pub cmdline: String,
}
