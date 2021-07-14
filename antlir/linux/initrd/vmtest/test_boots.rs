/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nix::sys::socket::{InetAddr, SockAddr};
use std::process::Command;

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
fn system_running() {
    assert_eq!("running", wait_for_systemd().trim());
}

#[test]
fn reaches_initrd_target() {
    wait_for_systemd();
    let out = String::from_utf8(
        Command::new("systemd-analyze")
            .arg("time")
            .output()
            .expect("failed to execute 'systemd-analyze time'")
            .stdout,
    )
    .expect("output not UTF-8");
    assert!(out.contains("initrd.target reached"));
}

#[test]
fn not_tainted() {
    wait_for_systemd();
    let out = String::from_utf8(
        Command::new("systemctl")
            .arg("show")
            .arg("--property")
            .arg("Tainted")
            .output()
            .expect("failed to execute 'systemd-analyze time'")
            .stdout,
    )
    .expect("output not UTF-8");
    assert_eq!("Tainted=", out.trim());
}

#[test]
fn got_ip_from_ra() {
    wait_for_systemd();
    let addrs = nix::ifaddrs::getifaddrs().unwrap();
    for ifaddr in addrs {
        println!("{} {}", ifaddr.interface_name, ifaddr.address.unwrap());
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
    panic!("could not find non-link-local address for eth0");
}
