/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;

use anyhow::Context;
use anyhow::Result;
use metalos_host_configs::host::HostConfig;
use serde::Serialize;
use service_config_generator_if::Input;
use service_config_generator_if::Output;
use state::State;
use thrift_wrapper::ThriftWrapper;

use crate::unit_file::Environment;

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GeneratedDropin {
    service: GeneratedServiceDropin,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GeneratedServiceDropin {
    environment: Environment,
}

impl From<service_config_generator_if::Dropin> for GeneratedDropin {
    fn from(d: service_config_generator_if::Dropin) -> Self {
        Self {
            service: GeneratedServiceDropin {
                environment: Environment(d.environment),
            },
        }
    }
}

pub(crate) fn evaluate_generator<P>(path: P) -> Result<Output>
where
    P: AsRef<OsStr>,
{
    let host_config = HostConfig::current().context("while loading HostConfig")?;
    match host_config {
        None => anyhow::bail!("no HostConfig found"),
        Some(h) => {
            let input = Input {
                host_identity: h.provisioning_config.identity.into_thrift(),
                deployment_runtime_config: h.runtime_config.deployment_specific.into_thrift(),
            };
            generator::evaluate(path, &input).map_err(|e| e.into())
        }
    }
}
