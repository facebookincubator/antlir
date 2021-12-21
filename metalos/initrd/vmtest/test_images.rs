/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Serialize;

use generator_lib::{Environment, ENVIRONMENT_FILENAME};

#[derive(Serialize)]
struct MinimalEnvironment {
    #[serde(rename = "METALOS_IMAGES_DIR")]
    images: PathBuf,

    #[serde(rename = "METALOS_OS_PKG")]
    pkg: String,
}
impl Environment for MinimalEnvironment {}

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
fn fetch_unit() -> Result<()> {
    let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let fake_root = Path::new("/unittest_root").join(format!("test_fetch_unit{:?}", ts));
    std::fs::create_dir_all(&fake_root)?;

    let env = MinimalEnvironment {
        images: fake_root.join("volume/image"),
        pkg: "metalos:1".to_string(),
    };

    env.write_systemd_env_file(
        Path::new("/run/systemd/generator/"),
        Path::new(ENVIRONMENT_FILENAME),
    )
    .context("failed to write environment file")?;

    assert_eq!("running", wait_for_systemd().trim());

    let mount_output = Command::new("mount")
        .arg("-t")
        .arg("btrfs")
        .arg("/dev/vda")
        .arg(&fake_root)
        .output()
        .expect("failed to start mount command");

    println!("{:#?}", mount_output);
    assert!(mount_output.status.success());

    let ls_output = Command::new("ls")
        .arg(&fake_root)
        .output()
        .expect("failed to start ls command");

    println!("{:#?}", ls_output);

    let fetch_output = Command::new("systemctl")
        .arg("start")
        .arg("metalos-fetch-image-rootfs.service")
        .output()
        .expect("failed to start metalos-fetch-image");

    println!("{:#?}", fetch_output);

    let journal = String::from_utf8(
        Command::new("journalctl")
            .arg("-u")
            .arg("metalos-fetch-image-rootfs.service")
            .output()
            .expect("failed to get journal output")
            .stdout,
    )
    .expect("output not utf-8");

    println!("journal output: {}", journal);

    assert!(fetch_output.status.success());

    let dir = fake_root.join("volume/image/rootfs/metalos/metalos:1/volume");
    assert!(dir.is_dir(), "{:?} is not a directory: {}", dir, journal);

    Ok(())
}
