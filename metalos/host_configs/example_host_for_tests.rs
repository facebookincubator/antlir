/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::host::HostConfig;
use metalos_host_configs::provisioning_config::{
    HostIdentity, Network, NetworkInterface, ProvisioningConfig, DNS,
};

pub fn example_host_for_tests() -> HostConfig {
    HostConfig {
        provisioning_config: ProvisioningConfig {
            identity: HostIdentity {
                id: format!("{:032x}", 1),
                hostname: "host001.01.abc0.facebook.com".to_owned(),
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
            },
            ..Default::default()
        },
        ..Default::default()
    }
}
