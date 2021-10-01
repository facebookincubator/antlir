/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::AddrParseError;

use derive_builder::Builder;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use starlark::values::{AllocValue, Heap, StarlarkAttrs, Value};

/// Host is the main entrypoint to the Starlark config generator runtime. It is
/// the top level struct that should contain all the structured information
/// about a host that is necessary for the config generators to materialize
/// config files. This is designed to (eventually) be serializable by an
/// external service and provided directly to a MetalOS host's initrd.
#[derive(
    Debug,
    Display,
    PartialEq,
    Eq,
    Clone,
    Deserialize,
    Serialize,
    StarlarkAttrs,
    Builder
)]
#[builder(setter(into), build_fn(validate = "Self::validate"))]
#[display(fmt = "{:?}", self)]
pub struct Host {
    // 32-char hex identifier
    pub id: String,
    pub hostname: String,
    pub network: Network,
    #[cfg(feature = "facebook")]
    #[builder(default)]
    pub facebook: crate::facebook::HostFacebook,
}
simple_data_struct!(Host);

impl HostBuilder {
    fn validate(&self) -> Result<(), String> {
        match &self.id {
            Some(id) => match id.len() == 32 {
                true => match id.chars().all(|c| c.is_ascii_hexdigit()) {
                    true => Ok(()),
                    false => Err("id must be exactly 32 hex chars".to_owned()),
                },
                false => Err("id must be exactly 32 hex chars".to_owned()),
            },
            None => Err("id must be set".to_owned()),
        }
    }
}

/// Wrap std::net::IpAddr to make it easy to expose to Starlark. Note that this
/// simply exposes the string value of the address, and does not provide any
/// additional information to Starlark. If that is necessary in the future, this
/// wrapper would need to have an implementation for StarlarkValue.
#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Serialize)]
pub struct IpAddr(std::net::IpAddr);

impl std::ops::Deref for IpAddr {
    type Target = std::net::IpAddr;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::str::FromStr for IpAddr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<IpAddr, AddrParseError> {
        Ok(Self(s.parse()?))
    }
}

impl<T> From<T> for IpAddr
where
    T: Into<std::net::IpAddr>,
{
    fn from(x: T) -> Self {
        Self(x.into())
    }
}

impl<'v> AllocValue<'v> for IpAddr {
    fn alloc_value(self, heap: &'v Heap) -> Value<'v> {
        heap.alloc(self.0.to_string())
    }
}

/// Top-level network settings.
#[derive(
    Debug,
    Display,
    PartialEq,
    Eq,
    Clone,
    Deserialize,
    Serialize,
    StarlarkAttrs,
    Builder
)]
#[builder(setter(into))]
#[display(fmt = "{:?}", self)]
pub struct Network {
    pub dns: DNS,
    pub interfaces: Vec<NetworkInterface>,
}
simple_data_struct!(Network);

/// Configuration for DNS resolvers
#[derive(
    Debug,
    Display,
    PartialEq,
    Eq,
    Clone,
    Deserialize,
    Serialize,
    StarlarkAttrs,
    Builder
)]
#[builder(setter(into))]
#[display(fmt = "{:?}", self)]
pub struct DNS {
    pub servers: Vec<IpAddr>,
    #[builder(default)]
    #[serde(default)]
    pub search_domains: Vec<String>,
}
simple_data_struct!(DNS);

/// Configuration for a single network interface, keyed by MAC Address.
#[derive(
    Debug,
    Display,
    PartialEq,
    Eq,
    Clone,
    Deserialize,
    Serialize,
    StarlarkAttrs,
    Builder
)]
#[builder(setter(into), build_fn(validate = "Self::validate"))]
#[display(fmt = "{:?}", self)]
pub struct NetworkInterface {
    pub mac: String,
    pub addrs: Vec<IpAddr>,
    #[builder(setter(strip_option))]
    pub name: Option<String>,
}
simple_data_struct!(NetworkInterface);

impl NetworkInterfaceBuilder {
    fn validate(&self) -> Result<(), String> {
        if self.addrs.as_ref().map(|a| a.is_empty()).unwrap_or(true) {
            return Err("must have at least one ip address in addrs".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starlark::assert::Assert;

    /// It doesn't make sense to implement Default for Host unless we are in a unit test
    impl Default for Host {
        fn default() -> Self {
            Host::builder()
                .id(format!("{:032x}", 42))
                .hostname("host001.01.abc0.facebook.com")
                .network(
                    Network::builder()
                        .dns(
                            DNS::builder()
                                .servers(vec!["2606:4700:4700::1111".parse().unwrap()])
                                .build()
                                .unwrap(),
                        )
                        .interfaces(vec![
                            NetworkInterface::builder()
                                .mac("00:00:00:00:00:01")
                                .addrs(vec!["2a03:2880:f103:181:face:b00c:0:25de".parse().unwrap()])
                                .name("eth0")
                                .build()
                                .unwrap(),
                        ])
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap()
        }
    }

    #[test]
    fn exposed_to_starlark() {
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("input", Host::default()));
        a.eq("input.hostname", "\"host001.01.abc0.facebook.com\"");
        let expected_dir = "\"hostname\", \"id\", \"network\"";
        if cfg!(feature = "facebook") {
            a.eq(
                "set(dir(input))",
                &format!("[\"facebook\", {}]", expected_dir),
            );
        } else {
            a.eq("set(dir(input))", &format!("[{}]", expected_dir));
        }
        a.eq("input.network.dns.servers", "[\"2606:4700:4700::1111\"]");
        a.eq(
            "input.network.interfaces[0].addrs",
            "[\"2a03:2880:f103:181:face:b00c:0:25de\"]",
        );
    }

    #[cfg(feature = "facebook")]
    #[test]
    fn facebook_exposed() {
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("input", Host::default()));
        a.eq("hasattr(input, \"facebook\")", "True");
    }
}
