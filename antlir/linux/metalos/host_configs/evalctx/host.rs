/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::AddrParseError;

use derive_builder::Builder;
use starlark::values::{AllocValue, Heap, Value};
use starlark_module::StarlarkAttrs;

/// Host is the main entrypoint to the Starlark config generator runtime. It is
/// the top level struct that should contain all the structured information
/// about a host that is necessary for the config generators to materialize
/// config files. This is designed to (eventually) be serializable by an
/// external service and provided directly to a MetalOS host's initrd.
#[derive(Debug, PartialEq, Eq, Clone, StarlarkAttrs, Builder)]
#[builder(setter(into))]
pub struct Host {
    pub hostname: String,
    pub network: Network,
}
simple_data_struct!(Host);

/// Wrap std::net::IpAddr to make it easy to expose to Starlark. Note that this
/// simply exposes the string value of the address, and does not provide any
/// additional information to Starlark. If that is necessary in the future, this
/// wrapper would need to have an implementation for StarlarkValue.
#[derive(Debug, PartialEq, Eq, Clone)]
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

impl<'v> AllocValue<'v> for IpAddr {
    fn alloc_value(self, heap: &'v Heap) -> Value<'v> {
        heap.alloc(self.0.to_string())
    }
}

/// Top-level network settings.
#[derive(Debug, PartialEq, Eq, Clone, StarlarkAttrs, Builder)]
#[builder(setter(into))]
pub struct Network {
    pub dns: DNS,
    pub interfaces: Vec<NetworkInterface>,
}
simple_data_struct!(Network);

/// Configuration for DNS resolvers
#[derive(Debug, PartialEq, Eq, Clone, StarlarkAttrs, Builder)]
#[builder(setter(into))]
pub struct DNS {
    pub servers: Vec<IpAddr>,
    #[builder(default)]
    pub search_domains: Vec<String>,
}
simple_data_struct!(DNS);

/// Configuration for a single network interface, keyed by MAC Address.
#[derive(Debug, PartialEq, Eq, Clone, StarlarkAttrs, Builder)]
#[builder(setter(into), build_fn(validate = "Self::validate"))]
pub struct NetworkInterface {
    pub mac: String,
    pub addrs: Vec<IpAddr>,
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
        a.eq("set(dir(input))", "[\"hostname\", \"network\"]");
        a.eq("input.network.dns.servers", "[\"2606:4700:4700::1111\"]");
        a.eq(
            "input.network.interfaces[0].addrs",
            "[\"2a03:2880:f103:181:face:b00c:0:25de\"]",
        );
    }
}
