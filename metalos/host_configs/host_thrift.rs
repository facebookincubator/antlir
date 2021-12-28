/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(feature = "facebook")]
pub use host_facebook as facebook;

impl Host {
    pub fn example_host_for_tests() -> Self {
        Host {
            id: format!("{:032x}", 1),
            hostname: "host001.01.abc0.facebook.com".to_owned(),
            root_pw_hash: "$0$unit_test_hash".to_string(),
            network: Network {
                dns: DNS {
                    servers: vec!["2606:4700:4700::1111".parse().unwrap()],
                    search_domains: vec![],
                },
                interfaces: vec![NetworkInterface {
                    mac: "00:00:00:00:00:01".to_owned(),
                    addrs: vec!["2a03:2880:f103:181:face:b00c:0:25de".parse().unwrap()],
                    name: Some("eth0".to_owned()),
                }],
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starlark::assert::Assert;

    #[test]
    fn test_exposed_to_starlark() {
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("input", Host::example_host_for_tests()));
        a.eq("input.hostname", "\"host001.01.abc0.facebook.com\"");
        a.eq("input.root_pw_hash", "\"$0$unit_test_hash\"");
        let expected_dir = "\"hostname\", \"id\", \"network\", \"root_pw_hash\"";
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

    #[cfg_attr(feature = "facebook", test)]
    fn facebook_exposed() {
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("input", Host::default()));
        a.eq("hasattr(input, \"facebook\")", "True");
    }
}
