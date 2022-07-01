/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;

use anyhow::Result;
use nix::sys::utsname::uname;
use serde::Serialize;

use crate::dropin::Environment;
use service_config_generator_if::Input;
use service_config_generator_if::Output;

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
    let uts = uname();
    let input = Input {
        kernel_version: uts.release().to_string(),
    };
    generator::evaluate(path, &input).map_err(|e| e.into())
}
