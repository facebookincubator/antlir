/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use thrift_wrapper::ThriftWrapper;

use crate::packages;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::runtime_config::RuntimeConfig)]
pub struct RuntimeConfig {
    #[cfg(facebook)]
    pub deployment_specific: crate::facebook::deployment_specific::DeploymentRuntimeConfig,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::runtime_config::Service)]
pub struct Service {
    pub svc: packages::Service,
    pub config_generator: Option<packages::ServiceConfigGenerator>,
}

impl Service {
    /// Path to metalos metadata dir
    pub fn metalos_dir(&self) -> Option<PathBuf> {
        self.svc.file_in_image("metalos")
    }

    /// Systemd unit name
    pub fn unit_name(&self) -> String {
        format!("{}.service", self.svc.name)
    }

    pub fn unit_file(&self) -> Option<PathBuf> {
        let path = self.metalos_dir().map(|d| d.join(self.unit_name()))?;
        match path.exists() {
            true => Some(path),
            false => None,
        }
    }
}
