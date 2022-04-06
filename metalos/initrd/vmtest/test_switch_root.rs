/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Test initrd features that boot through the switch-root into the image
/// This uses the regular initrd so that it goes through the regular boot
/// process, and this unit test is run inside a snapshot of the metalos base
/// image.
use anyhow::Result;
use systemd::{Systemd, WaitableSystemState};

async fn wait_for_systemd() -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let sd = Systemd::connect(log).await?;
    sd.wait(WaitableSystemState::Starting).await?;
    Ok(())
}

#[tokio::test]
async fn systemd_running() {
    wait_for_systemd().await.unwrap();
}

#[tokio::test]
async fn in_boot_snapshot() {
    wait_for_systemd().await.unwrap();
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .unwrap()
        .trim()
        // systemd's format specifier for boot id strips out dashes
        .replace("-", "");
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap();
    for line in mounts.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields[1] == "/" {
            // don't really care about the exact format, but the current boot id
            // should at least be present in the subvolume mounted at /
            assert!(
                fields[3].contains(&boot_id),
                "could not find boot id '{}' in subvol '{}'",
                boot_id,
                fields[3],
            );
            return;
        }
    }
    panic!("could not find / mount")
}
