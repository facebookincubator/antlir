/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! (Very thin) wrappers around the new Linux mount api

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use libc::AT_FDCWD;
use libc::AT_RECURSIVE;
use libc::AT_SYMLINK_NOFOLLOW;
use libc::c_char;
use libc::c_uint;
use rustix::fs::CWD;
use rustix::mount::FsMountFlags;
use rustix::mount::FsOpenFlags;
use rustix::mount::MountAttrFlags;
use rustix::mount::MoveMountFlags;
use rustix::mount::fsconfig_create;
use rustix::mount::fsmount;
use rustix::mount::fsopen;
use rustix::mount::move_mount;

// mount_setattr is not yet implemented in rustix, so we keep the raw syscall.
#[repr(C)]
#[allow(non_camel_case_types)]
struct mount_attr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

unsafe fn mount_setattr(
    dirfd: std::os::fd::RawFd,
    path: *const c_char,
    flags: c_uint,
    attr: &mount_attr,
) -> Result<(), std::io::Error> {
    unsafe {
        if libc::syscall(
            libc::SYS_mount_setattr,
            dirfd,
            path,
            flags,
            attr as *const _ as usize,
            std::mem::size_of::<mount_attr>(),
        ) == -1
        {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

pub(crate) fn make_mount_readonly(path: &Path) -> Result<()> {
    let path_c = CString::new(path.as_os_str().as_bytes()).context("while making CString path")?;
    unsafe {
        mount_setattr(
            AT_FDCWD,
            path_c.as_ptr(),
            (AT_SYMLINK_NOFOLLOW | AT_RECURSIVE) as u32,
            &mount_attr {
                attr_set: MountAttrFlags::MOUNT_ATTR_RDONLY.bits() as u64,
                attr_clr: 0,
                propagation: 0,
                userns_fd: 0,
            },
        )
    }
    .context("while making mount readonly")
}

/// Mount a new proc filesystem at `target` using the new mount API
/// (fsopen/fsconfig/fsmount/move_mount).
///
/// If `readonly` is true, the mount is created with MOUNT_ATTR_RDONLY. This is
/// needed as a fallback in non-init user namespaces where the kernel's
/// `mount_too_revealing()` check (fs/namespace.c) blocks read-write proc
/// mounts. The check calls `mnt_already_visible()` which iterates all existing
/// proc-type mounts and requires at least one to be "fully visible" (no
/// MNT_LOCKED children on non-empty directories, and compatible flags).
///
/// On systemd-nspawn hosts, the container's /proc has locked masking mounts
/// (on /proc/kmsg, /proc/sys/kernel/random/boot_id, etc.) making it "not fully
/// visible". However, /run/host/sandcastle-proc is mounted by Sandcastle
/// (https://fburl.com/code/3pkk22bj) as a read-only view of the parent
/// process's procfs for exactly this usage within antlir. Since that mount
/// has MNT_LOCK_READONLY, `mnt_already_visible()` only matches it for mounts
/// that also request MNT_READONLY. Mounting proc readonly allows this match,
/// producing a fresh proc for the correct PID namespace.
pub(crate) fn mount_proc(target: &Path, readonly: bool) -> Result<()> {
    // 1. fsopen("proc", FSOPEN_CLOEXEC) — create a filesystem context for proc
    let fs_fd = fsopen("proc", FsOpenFlags::FSOPEN_CLOEXEC).context("fsopen(\"proc\") failed")?;

    // 2. fsconfig_create(fs_fd) — create the superblock
    fsconfig_create(&fs_fd).context("fsconfig_create failed")?;

    // 3. fsmount(fs_fd, FSMOUNT_CLOEXEC, attr_flags) — create a detached mount
    let mut attr_flags = MountAttrFlags::MOUNT_ATTR_NOSUID
        | MountAttrFlags::MOUNT_ATTR_NODEV
        | MountAttrFlags::MOUNT_ATTR_NOEXEC;
    if readonly {
        attr_flags |= MountAttrFlags::MOUNT_ATTR_RDONLY;
    }
    let mnt_fd =
        fsmount(&fs_fd, FsMountFlags::FSMOUNT_CLOEXEC, attr_flags).context("fsmount failed")?;

    // 4. move_mount(mnt_fd, "", AT_FDCWD, target, MOVE_MOUNT_F_EMPTY_PATH) — attach it
    move_mount(
        &mnt_fd,
        "",
        CWD,
        target,
        MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
    )
    .context("move_mount failed")?;

    Ok(())
}
