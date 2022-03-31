/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::str::FromStr;

use anyhow::{Context, Error, Result};
use kernel_cmdline::KernelCmdArgs;
use reqwest::Url;
use serde::{de, Deserialize, Deserializer};

use crate::kernel_cmdline::MetalosCmdline;

#[derive(Default, Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub event_backend: EventBackend,
}

impl Config {
    /// Some config options can be overridden by the kernel cmdline. The default
    /// values are first deserialized from the config file
    /// (/etc/metalctl.toml), and then any args present on the kernel cmdline
    /// are processed.
    pub fn apply_kernel_cmdline_overrides(&mut self) -> Result<()> {
        self.apply_overrides(MetalosCmdline::from_proc_cmdline()?)
    }

    fn apply_overrides(&mut self, cmdline: MetalosCmdline) -> Result<()> {
        if let Some(uri) = cmdline.event_backend_base_uri {
            self.event_backend.event_backend_base_uri = uri;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct EventBackendBaseUri(Url);

impl FromStr for EventBackendBaseUri {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        s.parse().map(Self).context("not valid url")
    }
}

impl<'de> Deserialize<'de> for EventBackendBaseUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventBackend {
    pub event_backend_base_uri: EventBackendBaseUri,
}

impl EventBackend {
    pub fn event_backend_base_uri(&self) -> &Url {
        &self.event_backend_base_uri.0
    }
}

impl Default for EventBackend {
    fn default() -> Self {
        Self {
            event_backend_base_uri: "https://metalos/sendEvent".parse().unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use kernel_cmdline::KernelCmdArgs;

    use super::{Config, MetalosCmdline};
    #[test]
    fn overrides() -> Result<()> {
        let mut config = Config::default();
        assert_eq!(
            "https://metalos/sendEvent",
            config.event_backend.event_backend_base_uri().to_string()
        );
        let cmdline = MetalosCmdline::from_kernel_args(
            "metalos.event_backend_base_uri=\"https://event-host/sendEvent\"",
        )?;
        config.apply_overrides(cmdline)?;
        assert_eq!(
            "https://event-host/sendEvent",
            config.event_backend.event_backend_base_uri().to_string()
        );
        Ok(())
    }
}
