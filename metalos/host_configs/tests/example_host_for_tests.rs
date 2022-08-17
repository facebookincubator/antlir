/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::host::HostConfig;
use metalos_thrift_host_configs::boot_config::BootConfig;
use metalos_thrift_host_configs::boot_config::Kernel;
use metalos_thrift_host_configs::packages::Format;
use metalos_thrift_host_configs::packages::Kind;
use metalos_thrift_host_configs::packages::Package;
use metalos_thrift_host_configs::packages::PackageId;
use metalos_thrift_host_configs::provisioning_config::DiskConfiguration;
use metalos_thrift_host_configs::provisioning_config::EventBackend;
use metalos_thrift_host_configs::provisioning_config::EventSource;
use metalos_thrift_host_configs::provisioning_config::HostIdentity;
use metalos_thrift_host_configs::provisioning_config::Network;
use metalos_thrift_host_configs::provisioning_config::NetworkAddress;
use metalos_thrift_host_configs::provisioning_config::NetworkAddressMode;
use metalos_thrift_host_configs::provisioning_config::NetworkInterface;
use metalos_thrift_host_configs::provisioning_config::NetworkInterfaceType;
use metalos_thrift_host_configs::provisioning_config::ProvisioningConfig;
use metalos_thrift_host_configs::provisioning_config::RootDiskConfiguration;
use metalos_thrift_host_configs::provisioning_config::SingleDiskSerial;
use metalos_thrift_host_configs::provisioning_config::DNS;
use metalos_thrift_host_configs::runtime_config::RuntimeConfig;

pub fn example_host_for_tests() -> HostConfig {
    match HostConfig::try_from(metalos_thrift_host_configs::host::HostConfig {
        provisioning_config: ProvisioningConfig {
            identity: HostIdentity {
                id: format!("{:032x}", 1),
                hostname: "host001.01.abc0.facebook.com".to_owned(),
                network: Network {
                    dns: DNS {
                        servers: vec!["2606:4700:4700::1111".to_owned()],
                        search_domains: vec![],
                    },
                    interfaces: vec![NetworkInterface {
                        mac: "00:00:00:00:00:01".to_owned(),
                        addrs: None,
                        name: Some("eth0".to_owned()),
                        essential: true,
                        structured_addrs: vec![NetworkAddress{
                            addr: "2a03:2880:f103:181:face:b00c:0:25de".to_owned(),
                            prefix_length: 64,
                            mode: NetworkAddressMode::PRIMARY}],
                        interface_type: NetworkInterfaceType::FRONTEND,

                    },
                    NetworkInterface {
                        mac: "00:00:00:00:00:03".to_owned(),
                        addrs: None,
                        name: Some("eth8".to_owned()),
                        essential: false,
                        structured_addrs: vec![],
                        interface_type: NetworkInterfaceType::FRONTEND,

                    },
                    NetworkInterface {
                        mac: "00:00:00:00:00:04".to_owned(),
                        addrs: None,
                        name: Some("beth3".to_owned()),
                        essential: false,
                        structured_addrs: vec![NetworkAddress{
                            addr: "2a03:2880:f103:181:face:b00c:a1:0".to_owned(),
                            prefix_length: 64,
                            mode: NetworkAddressMode::PRIMARY}],
                        interface_type: NetworkInterfaceType::BACKEND,

                    }],
                    primary_interface: NetworkInterface {
                        mac: "00:00:00:00:00:01".to_owned(),
                        addrs: None,
                        name: Some("eth0".to_owned()),
                        essential: true,
                        structured_addrs: vec![NetworkAddress{
                            addr: "2a03:2880:f103:181:face:b00c:0:25de".to_owned(),
                            prefix_length: 64,
                            mode: NetworkAddressMode::PRIMARY}],
                        interface_type: NetworkInterfaceType::FRONTEND,
                    },
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
            root_disk_config: RootDiskConfiguration::single_serial(SingleDiskSerial {
                serial: "deadbeef".into(),
                config: DiskConfiguration { sector_size: 512 },
            }),
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
