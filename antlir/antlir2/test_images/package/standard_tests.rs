/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::path::Path;

#[cfg(feature = "dot_meta")]
use buck_label::Label;
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
use cap_std::fs::OpenOptionsExt;
#[cfg(feature = "xattr")]
use libcap::FileExt as _;
#[cfg(feature = "xattr")]
use maplit::btreemap;
use nix::fcntl::readlinkat;
#[cfg(feature = "xattr")]
use xattr::FileExt as _;

mod stub;

#[test]
fn antlir2_large_file_256m() {
    let package = stub::open();
    let large_file = package
        .read("antlir2-large-file-256M")
        .expect("failed to read");
    assert_eq!(large_file.len(), 256 * 1024 * 1024);
    let line = b"antlir2-large-file\n";
    let mut idx = 0;
    while idx < large_file.len() {
        let chunk_len = std::cmp::min(idx + line.len(), large_file.len()) - idx;
        assert_eq!(&large_file[idx..idx + chunk_len], &line[..chunk_len]);
        idx += line.len();
    }
}

#[cfg(feature = "xattr")]
#[test]
fn xattrs() {
    let package = stub::open();
    let f = package
        .open("i-have-xattrs")
        .expect("failed to open")
        .into_std();
    let xattrs: BTreeMap<OsString, Vec<u8>> = f
        .list_xattr()
        .expect("failed to list xattrs")
        .map(|k| {
            (
                k.clone(),
                f.get_xattr(&k)
                    .expect("failed to read xattr value")
                    .expect("xattr missing"),
            )
        })
        .collect();
    assert_eq!(
        xattrs,
        btreemap! {
            "user.foo".into() => b"bar".to_vec(),
            "user.baz".into() => b"qux".to_vec(),
        }
    );
}

#[cfg(feature = "xattr")]
#[test]
fn capabilities() {
    // this is basicaly the same as xattrs, but ensures that the
    // security.capability xattr is packaged correctly
    let package = stub::open();
    let f = package
        .open("i-have-caps")
        .expect("failed to open")
        .into_std();
    let caps = f
        .get_capabilities()
        .expect("failed to read capabilities")
        .expect("capabilities missing");
    assert_eq!(caps.to_string(), "= cap_setuid+ep");
}

#[test]
fn ownership() {
    let package = stub::open();
    let meta = package
        .metadata("i-am-owned-by-nonstandard")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 42);
    assert_eq!(meta.gid(), 43);
}

#[test]
fn locked_permissions() {
    let package = stub::open();
    let meta = package
        .metadata("only-readable-by-root")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0);
}

#[test]
fn executable() {
    let package = stub::open();
    let meta = package
        .metadata("default-dir/executable")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0o555);
}

#[test]
fn directory() {
    let package = stub::open();
    let meta = package.metadata("default-dir").expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0o755);
}

#[test]
fn file_absolute_symlink() {
    let package = stub::open();
    let target =
        readlinkat(package.as_raw_fd(), "absolute-file-symlink").expect("failed to readlink");
    assert_eq!(target, Path::new("/default-dir/executable"));
}

#[test]
fn file_relative_symlink() {
    let package = stub::open();
    let target = readlinkat(package.as_raw_fd(), "default-dir/relative-file-symlink")
        .expect("failed to readlink");
    assert_eq!(target, Path::new("executable"));
}

#[test]
fn dir_absolute_symlink() {
    let package = stub::open();
    let target =
        readlinkat(package.as_raw_fd(), "absolute-dir-symlink").expect("failed to readlink");
    assert_eq!(target, Path::new("/default-dir"));
}

#[test]
fn dir_relative_symlink() {
    let package = stub::open();
    let target =
        readlinkat(package.as_raw_fd(), "relative-dir-symlink").expect("failed to readlink");
    assert_eq!(target, Path::new("default-dir"));
}

#[cfg(feature = "dot_meta")]
#[test]
fn dot_meta() {
    let package = stub::open();
    let target = package
        .read_to_string(".meta/target")
        .expect("failed to read /.meta/target");
    let label: Label = target
        .trim()
        .parse()
        .expect(".meta/target is not a valid label");
    assert!(
        label
            .package()
            .starts_with("antlir/antlir2/test_images/package"),
        "label '{label}' not in expected package"
    );
}
