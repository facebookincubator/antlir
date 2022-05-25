/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::host::HostConfig;
use metalos_thrift_host_configs::boot_config::{BootConfig, Kernel};
use metalos_thrift_host_configs::packages::{Format, Kind, Package, PackageId};
use metalos_thrift_host_configs::provisioning_config::{
    EventBackend, EventSource, HostIdentity, Network, NetworkInterface, ProvisioningConfig, DNS,
};
use metalos_thrift_host_configs::runtime_config::RuntimeConfig;

pub fn example_host_for_tests() -> HostConfig {
    match HostConfig::try_from(metalos_thrift_host_configs::host::HostConfig {
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
            gpt_root_disk: Package {
                name: "metalos.gpt-root-disk".into(),
                id: PackageId::uuid("deadbeefdeadbeefdeadbeefdeadbeef".into()),
                kind: Kind::GPT_ROOT_DISK,
                format: Format::FILE,
                ..Default::default()
            },
            imaging_initrd: Package {
                name: "metalos.imaging-initrd".into(),
                id: PackageId::uuid("deadbeefdeadbeefdeadbeefdeadbeef".into()),
                kind: Kind::IMAGING_INITRD,
                format: Format::FILE,
                ..Default::default()
            },
            event_backend: EventBackend {
                source: EventSource::asset_id(1),
                base_uri: "https://metalos/send-event".into(),
            },
            #[cfg(facebook)]
            deployment_specific:
                metalos_host_configs::facebook::deployment_specific::DeploymentProvisioningConfig::Metalos(
                    Default::default(),
                ).into(),
            ..Default::default()
        },
        boot_config: BootConfig {
            rootfs: Package {
                name: "metalos.rootfs".into(),
                id: PackageId::uuid("deadbeefdeadbeefdeadbeefdeadbeef".into()),
                kind: Kind::ROOTFS,
                format: Format::SENDSTREAM,
                ..Default::default()
            },
            kernel: Kernel {
                pkg: Package {
                    name: "metalos.rootfs".into(),
                    id: PackageId::uuid("deadbeefdeadbeefdeadbeefdeadbeef".into()),
                    kind: Kind::KERNEL,
                    format: Format::SENDSTREAM,
                    ..Default::default()
                },
                cmdline: "kernelcmdline".into(),
            },
            initrd: Package {
                name: "metalos.initrd".into(),
                id: PackageId::uuid("deadbeefdeadbeefdeadbeefdeadbeef".into()),
                kind: Kind::INITRD,
                format: Format::FILE,
                ..Default::default()
            },
            #[cfg(facebook)]
            deployment_specific:
                metalos_host_configs::facebook::deployment_specific::DeploymentBootConfig::Metalos(
                    Default::default(),
                ).into(),
            ..Default::default()
        },
        runtime_config: RuntimeConfig {
            #[cfg(facebook)]
            deployment_specific:
                metalos_host_configs::facebook::deployment_specific::DeploymentRuntimeConfig::Metalos(
                    Default::default(),
                ).into(),
            ..Default::default()
        },
        ..Default::default()
    }) {
        Ok(h) => h,
        Err(e) => panic!("{}", e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        let _ = super::example_host_for_tests();
    }
}
