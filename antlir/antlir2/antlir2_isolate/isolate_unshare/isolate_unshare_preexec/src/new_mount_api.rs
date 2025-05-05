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
use rustix::mount::MountAttrFlags;

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
