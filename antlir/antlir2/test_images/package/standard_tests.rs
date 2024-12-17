/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "dot_meta")]
use buck_label::Label;
use cap_std::fs::Dir;
use cap_std::fs::DirEntry;
use cap_std::fs::MetadataExt;
#[cfg(feature = "xattr")]
use libcap::FileExt as _;
#[cfg(feature = "xattr")]
use maplit::btreemap;
use nix::fcntl::readlinkat;
use pretty_assertions::assert_eq;
#[cfg(feature = "xattr")]
use xattr::FileExt as _;

trait Stub {
    fn open() -> Dir;
    fn absolute_symlink_root() -> &'static Path {
        Path::new("/")
    }
}

mod stub;
use stub::StubImpl;

#[test]
fn antlir2_large_file_256m() {
    let package = StubImpl::open();
    let large_file = package
        .read("antlir2-large-file-256M")
        .expect("failed to read");
    // this line is present 3 times in the file:
    let line = b"antlir2-large-file\n";
    assert_eq!(large_file.len(), (256 * 1024 * 1024) + (line.len() * 3));
    // right at the beginning
    assert_eq!(&large_file[..line.len()], line);
    // after 128M of random bytes
    assert_eq!(
        &large_file
            [line.len() + (128 * 1024 * 1024)..line.len() + (128 * 1024 * 1024) + line.len()],
        line
    );
    // at the end
    assert_eq!(&large_file[large_file.len() - line.len()..], line);
}

#[cfg(feature = "xattr")]
#[test]
fn xattrs() {
    let package = StubImpl::open();
    let f = package
        .open("i-have-xattrs")
        .expect("failed to open")
        .into_std();
    let xattrs: std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> = f
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
    let package = StubImpl::open();
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
    let package = StubImpl::open();
    let meta = package
        .metadata("i-am-owned-by-nonstandard")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 42);
    assert_eq!(meta.gid(), 43);
}

#[test]
fn locked_permissions() {
    let package = StubImpl::open();
    let meta = package
        .metadata("only-readable-by-root")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0);
}

#[test]
fn executable() {
    let package = StubImpl::open();
    let meta = package
        .metadata("default-dir/executable")
        .expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0o555);
}

#[test]
fn directory() {
    let package = StubImpl::open();
    let meta = package.metadata("default-dir").expect("failed to stat");
    assert_eq!(meta.uid(), 0);
    assert_eq!(meta.gid(), 0);
    assert_eq!(meta.mode() & 0o777, 0o755);
    assert!(meta.file_type().is_dir())
}

#[test]
fn file_absolute_symlink() {
    let package = StubImpl::open();
    let target =
        readlinkat(Some(package.as_raw_fd()), "absolute-file-symlink").expect("failed to readlink");
    assert_eq!(
        target,
        StubImpl::absolute_symlink_root().join("default-dir/executable")
    );
}

#[test]
fn file_relative_symlink() {
    let package = StubImpl::open();
    let target = readlinkat(
        Some(package.as_raw_fd()),
        "default-dir/relative-file-symlink",
    )
    .expect("failed to readlink");
    assert_eq!(target, Path::new("executable"));
}

#[test]
fn dir_absolute_symlink() {
    let package = StubImpl::open();
    let target =
        readlinkat(Some(package.as_raw_fd()), "absolute-dir-symlink").expect("failed to readlink");
    assert_eq!(
        target,
        StubImpl::absolute_symlink_root().join("default-dir")
    );
}

#[test]
fn dir_relative_symlink() {
    let package = StubImpl::open();
    let target =
        readlinkat(Some(package.as_raw_fd()), "relative-dir-symlink").expect("failed to readlink");
    assert_eq!(target, Path::new("default-dir"));
}

#[test]
fn hardlink() {
    let package = StubImpl::open();
    let hardlink = package
        .open_dir("hardlink")
        .expect("failed to open /hardlink dir");
    let hello = hardlink.metadata("hello").expect("failed stat hello");
    let aloha = hardlink.metadata("aloha").expect("failed stat aloha");
    // Certain filesystems may not report the inode as the same when mounted
    // (specifically, erofsfuse at the time of this writing), but nlink will
    // still be correct
    assert_eq!(hello.nlink(), 2);
    assert_eq!(aloha.nlink(), 2);
    #[cfg(feature = "hardlink_ino_eq")]
    assert_eq!(hello.ino(), aloha.ino());
}

#[cfg(feature = "dot_meta")]
#[test]
fn dot_meta() {
    let package = StubImpl::open();
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

#[test]
fn no_unexpected_files() {
    let package = StubImpl::open();
    let mut all_files = BTreeSet::new();
    let mut queue: VecDeque<(PathBuf, DirEntry)> = package
        .entries()
        .expect("failed to read root")
        .map(|e| e.expect("failed to read root"))
        .map(|e| (e.file_name().into(), e))
        .collect();
    while let Some((path, entry)) = queue.pop_front() {
        let meta = entry.metadata().expect("failed to stat");
        if meta.is_dir() {
            queue.extend(
                entry
                    .open_dir()
                    .expect("failed to open as dir")
                    .entries()
                    .expect("failed to read dir")
                    .map(|e| e.expect("failed to read dir"))
                    .map(|e| (path.join(e.file_name()), e)),
            );
        }
        all_files.insert(path);
    }
    let mut expected_files: BTreeSet<_> = [
        "absolute-dir-symlink",
        "absolute-file-symlink",
        "antlir2-large-file-256M",
        "default-dir",
        "default-dir/executable",
        "default-dir/relative-file-symlink",
        "hardlink",
        "hardlink/aloha",
        "hardlink/hello",
        "i-am-owned-by-nonstandard",
        "i-have-caps",
        "i-have-xattrs",
        "only-readable-by-root",
        "relative-dir-symlink",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    #[cfg(feature = "dot_meta")]
    {
        expected_files.insert(".meta".into());
        expected_files.insert(".meta/target".into());
    }
    #[cfg(feature = "format_ext3")]
    expected_files.insert("lost+found".into());

    assert_eq!(expected_files, all_files);
}
