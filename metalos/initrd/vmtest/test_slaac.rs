/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use nix::sys::socket::{InetAddr, SockAddr};

fn wait_for_systemd() -> String {
    String::from_utf8(
        Command::new("systemctl")
            .arg("is-system-running")
            .arg("--wait")
            .output()
            .expect("failed to execute 'systemctl is-system-running'")
            .stdout,
    )
    .expect("output not UTF-8")
}

#[test]
fn got_ip_from_ra() {
    wait_for_systemd();
    // give it a few tries, since the ip might not be configured immediately if
    // the RA is delayed for whatever reason
    for attempt in 0..3 {
        let out = String::from_utf8(
            Command::new("systemctl")
                .arg("status")
                .arg("systemd-networkd-wait-online.service")
                .output()
                .expect("failed to check systemctl status")
                .stdout,
        )
        .expect("output not UTF-8");
        println!("attempt {}: systemd-networkd-wait-online: {}", attempt, out);

        let addrs = nix::ifaddrs::getifaddrs().unwrap();
        for ifaddr in addrs {
            println!(
                "attempt {}: {} {}",
                attempt,
                ifaddr.interface_name,
                ifaddr.address.unwrap()
            );
            if ifaddr.interface_name != "eth0" {
                continue;
            }
            if ifaddr.address.is_none() {
                continue;
            }
            if let SockAddr::Inet(InetAddr::V6(addr)) = ifaddr.address.unwrap() {
                if addr.sin6_scope_id != 0 {
                    // link-local
                    continue;
                }
                // this is a static ip as set by vmtest, ignore it and hope to
                // find the slaac one later
                if "[fd00::2]:0" == ifaddr.address.unwrap().to_string() {
                    continue;
                }
                assert_eq!(
                    "[fd00::200:ff:fe00:1]:0",
                    ifaddr.address.unwrap().to_string()
                );
                return;
            }
        }
        sleep(Duration::from_millis(250));
    }
    panic!("could not find slaac-configured address for eth0");
}

#[test]
fn is_eth_network_drop_in_in_place() {
    wait_for_systemd();

    assert!(Path::new("/usr/lib/systemd/network/50-eth.network").exists());
    let static_unit = std::fs::read_to_string("/usr/lib/systemd/network/50-eth.network")
        .expect("Can't read /usr/lib/systemd/network/50-eth.network file");

    assert!(
        !static_unit.contains("MACAddress=00:00:00:00:00:01"),
        "{}",
        static_unit
    );

    assert_eq!(
        std::fs::read_to_string("/usr/lib/systemd/network/50-eth.network.d/match.conf")
            .expect("Can't read /usr/lib/systemd/network/50-eth.network.d/match.conf file"),
        "\
        [Match]\n\
        Name=eth*\n\
        MACAddress=00:00:00:00:00:01\n\
        "
    );
}
