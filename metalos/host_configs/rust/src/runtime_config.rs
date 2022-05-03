/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::packages;
use thrift_wrapper::ThriftWrapper;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::runtime_config::RuntimeConfig)]
pub struct RuntimeConfig {
    #[cfg(facebook)]
    deployment_specific: crate::facebook::deployment_specific::DeploymentRuntimeConfig,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::runtime_config::Service)]
pub struct Service {
    pub svc: packages::Service,
    pub config_generator: Option<packages::ServiceConfigGenerator>,
}
