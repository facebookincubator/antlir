/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use antlir2_working_volume::WorkingVolume;
use nix::dir::Dir;
use nix::sys::stat::utimes;
use nix::sys::time::TimeVal;

extern "C" {
    pub fn getuid() -> u32;
}

#[test]
fn test_user_is_unprivileged() {
    assert_ne!(unsafe { getuid() }, 0);
}

#[test]
fn creates_new_volume() {
    std::env::set_current_dir("/empty_repo").expect("failed to chdir to /empty_repo");
    let wv = WorkingVolume::ensure("antlir2-out".into()).expect("failed to create working volume");
    assert!(wv.path().exists());
}

#[test]
fn handles_existing_volume() {
    std::env::set_current_dir("/repo").expect("failed to chdir to /repo");
    assert!(Path::new("antlir2-out").exists());
    let wv = WorkingVolume::ensure("antlir2-out".into()).expect("failed to create working volume");
    assert!(wv.path().exists());
}

#[test]
fn keepalive_simple() {
    std::env::set_current_dir("/repo").expect("failed to chdir to /repo");
    let wv = WorkingVolume::ensure("antlir2-out".into()).expect("failed to create working volume");
    let path = wv.allocate_new_path().expect("failed to allocate new path");
    std::fs::create_dir(&path).expect("failed to create dir");
    let path_canonical = path.canonicalize().expect("failed to canonicalize");
    std::os::unix::fs::symlink(path_canonical, "/links/link").expect("failed to symlink");

    // make sure it looks old enough
    utimes(&path, &TimeVal::new(1234, 0), &TimeVal::new(1234, 0))
        .expect("failed to set times on dir");

    wv.keep_path_alive(&path, Path::new("/links/link"))
        .expect("failed to keep path alive");

    wv.collect_garbage().expect("failed to collect garbage");
    assert!(path.exists(), "path should still exist after first gc");

    // now delete the symlink, gc should remove the dir

    std::fs::remove_file("/links/link").expect("failed to remove symlink");
    wv.collect_garbage().expect("failed to collect garbage");
    assert!(!path.exists(), "path should be deleted by second gc");
}
