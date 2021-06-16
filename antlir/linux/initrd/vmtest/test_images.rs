/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
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
fn systemd_unit() {
    assert_eq!("running", wait_for_systemd().trim());
    let escaped_url = r"http:--vmtest\x2dhost:8000-package-metalos:1";
    Command::new("systemctl")
        .arg("start")
        .arg(format!("antlir-fetch-image@{}.service", escaped_url))
        .spawn()
        .expect("failed to start antlir-fetch-image")
        .wait()
        .expect("antlir-fetch-image service failed");

    let dir = Path::new("/sysroot/var/lib/antlir/image/")
        .join(&escaped_url)
        .join("volume");
    let journal = String::from_utf8(
        Command::new("journalctl")
            .arg("-u")
            .arg(format!("antlir-fetch-image@{}.service", escaped_url))
            .output()
            .expect("failed to get journal output")
            .stdout,
    )
    .expect("output not utf-8");
    assert!(dir.is_dir(), "{:?} is not a directory: {}", dir, journal);
}
