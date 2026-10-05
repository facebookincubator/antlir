/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use nix::fcntl::OFlag;
use nix::pty::openpty;
use nix::sys::termios::SetArg;
use nix::sys::termios::cfmakeraw;
use nix::sys::termios::tcgetattr;
use nix::sys::termios::tcsetattr;

#[test]
fn dev_pts_is_devpts() {
    let statfs = rustix::fs::statfs("/dev/pts").expect("failed to statfs /dev/pts");
    assert_eq!(statfs.f_type, 0x1cd1, "/dev/pts is not a devpts mount"); // DEVPTS_SUPER_MAGIC
}

/// The pty slave must show up in this container's /dev/pts, which only happens
/// when /dev/ptmx allocates against the container's own devpts instance.
#[test]
fn openpty_slave_is_visible_and_usable() {
    let pty = openpty(None, None).expect("openpty failed");
    let slave_path: PathBuf =
        std::fs::read_link(format!("/proc/self/fd/{}", pty.slave.as_raw_fd()))
            .expect("failed to resolve pty slave path");
    assert!(
        slave_path.starts_with("/dev/pts"),
        "pty slave {} is not under /dev/pts",
        slave_path.display()
    );

    // Reopen the slave by name, the way a child process attaching to the
    // terminal would, and round-trip bytes through it. O_NOCTTY: becoming the
    // controlling terminal would SIGHUP the test runner when the master closes.
    let mut slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(OFlag::O_NOCTTY.bits())
        .open(&slave_path)
        .unwrap_or_else(|e| panic!("failed to open {}: {e}", slave_path.display()));
    let mut termios = tcgetattr(&slave).expect("tcgetattr failed");
    cfmakeraw(&mut termios);
    tcsetattr(&slave, SetArg::TCSANOW, &termios).expect("tcsetattr failed");

    let mut master = std::fs::File::from(pty.master);
    master.write_all(b"ping").expect("write to master failed");
    let mut buf = [0u8; 4];
    slave.read_exact(&mut buf).expect("read from slave failed");
    assert_eq!(&buf, b"ping");
}

#[test]
fn std_stream_symlinks() {
    for (link, target) in [
        ("/dev/stdin", "/proc/self/fd/0"),
        ("/dev/stdout", "/proc/self/fd/1"),
        ("/dev/stderr", "/proc/self/fd/2"),
    ] {
        let actual = std::fs::read_link(link).unwrap_or_else(|e| panic!("{link}: {e}"));
        assert_eq!(actual, Path::new(target), "{link}");
    }
}
