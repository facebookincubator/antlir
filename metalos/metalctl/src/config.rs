/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::str::FromStr;

use anyhow::{bail, Context, Error, Result};
use hyper::Uri;
use serde::{de, Deserialize, Deserializer};

use crate::kernel_cmdline::MetalosCmdline;

#[derive(Debug, PartialEq)]
pub struct PackageFormatUri(String);

impl FromStr for PackageFormatUri {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if !s.contains("{package}") {
            bail!("package_format_uri must contain the placeholder '{package}'");
        }
        match s.replace("{package}", "placeholder").parse::<Uri>() {
            Ok(u) => {
                match u.scheme_str() {
                    Some("http") | Some("https") => {}
                    _ => bail!("package_format_uri must be http(s) only"),
                };
            }
            Err(e) => {
                bail!("'{}' is not a valid URL: {}", s, e);
            }
        }
        Ok(Self(s.to_string()))
    }
}

impl PackageFormatUri {
    fn uri<S: AsRef<str>>(&self, package: S) -> Result<Uri> {
        let uri = self.0.replace("{package}", package.as_ref());
        uri.parse()
            .with_context(|| format!("'{}' is not a valid url", uri))
    }
}

impl<'de> Deserialize<'de> for PackageFormatUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

#[derive(Default, Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub download: Download,
}

impl Config {
    /// Some config options can be overridden by the kernel cmdline. The default
    /// values are first deserialized from the config file
    /// (/etc/metalctl.toml), and then any args present on the kernel cmdline
    /// are processed.
    pub fn apply_kernel_cmdline_overrides(&mut self) -> Result<()> {
        self.apply_overrides(MetalosCmdline::from_kernel()?)
    }

    fn apply_overrides(&mut self, cmdline: MetalosCmdline) -> Result<()> {
        if let Some(uri) = cmdline.package_format_uri {
            self.download.package_format_uri = uri;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Download {
    package_format_uri: PackageFormatUri,
}

impl Download {
    pub fn package_uri<S: AsRef<str>>(&self, package: S) -> Result<Uri> {
        self.package_format_uri.uri(package)
    }
}

impl Default for Download {
    fn default() -> Self {
        Self {
            package_format_uri: "https://metalos/package/{package}".parse().unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Config, MetalosCmdline};
    #[test]
    fn overrides() -> Result<()> {
        let mut config = Config::default();
        assert_eq!(
            config.download.package_format_uri.0,
            "https://metalos/package/{package}"
        );
        let cmdline: MetalosCmdline =
            "metalos.package_format_uri=\"https://package-host/pkg/{package}\"".parse()?;
        config.apply_overrides(cmdline)?;
        assert_eq!(
            config.download.package_format_uri.0,
            "https://package-host/pkg/{package}"
        );
        Ok(())
    }
}
