/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This is a helper library for unsharing the current process into a new,
//! unprivileged user namespace.
//! This is a little bit of a tricky dance that requires a few unsafe `fork()`s
//! and pipe based communication to accomplish the following flow:
//!
//! ┌────────────┐    ┌───────┐       ┌───────┐
//! │Main Process│    │Child 1│       │Child 2│
//! └─────┬──────┘    └───┬───┘       └───┬───┘
//!       │               │               │
//!       │    fork()     │               │
//!       │──────────────>│               │
//!       │               │               │
//!       │"I've unshared"│               │
//!       │──────────────>│               │
//!       │               │               │
//!       │               │    fork()     │
//!       │               │──────────────>│
//!       │               │               │
//!       │               │exec(newgidmap)│
//!       │               │<──────────────│
//!       │               │               │
//!       │        exec(newuidmap)        │
//!       │<──────────────────────────────│
//! ┌─────┴──────┐    ┌───┴───┐       ┌───┴───┐
//! │Main Process│    │Child 1│       │Child 2│
//! └────────────┘    └───────┘       └───────┘
//!
//! 1. Main Process starts in the initial user namespace. It forks Child 1 (also
//! in the initial user namespace).
//!
//! 2. Main Process unshares itself into a new user namespace. At this point,
//! the new user namespace has no IDs mapped into it.
//!
//! 3. Main Process closes the write end of the pipe it gave to Child 1 to
//! indicate that Main Process has created the new user namespace.
//!
//! 4. Child 1 forks Child 2 (also in the initial user namespace).
//!
//! 5. Child 2 execs /usr/bin/newgidmap to map GIDs into Main Process's new user
//! namespace.
//!
//! 6. Child 1 execs /usr/bin/newuidmap to map UIDs into Main Process's new user
//! namespace.
//!
//! 7. Main Process gets a 0 return code from Child 1 and continues its
//! execution. Main Process's user namespace now has a full range of UIDs and
//! GIDs mapped into it.

// This does a few `fork()`s with logic afterwards so we have to be careful not
// to accidentally do any dynamic memory allocation. An easy way to accomplish
// that is just using no_std.
#![no_std]

// In case jemalloc is used (the default in fbcode), this disables background
// threads which would prevent unsharing into a userns.
#[no_mangle]
pub static malloc_conf: &[u8] = b"background_thread:false\0";

use core::ffi::CStr;

use nix::errno::Errno;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::sys::wait::waitpid;
use nix::sys::wait::WaitStatus;
use nix::unistd::fork;
use nix::unistd::pipe;
use nix::unistd::ForkResult;
use nix::Result;

#[derive(Copy, Clone)]
pub struct Map<'a> {
    pub outside_root: &'a CStr,
    pub outside_sub_start: &'a CStr,
    pub len: &'a CStr,
}

pub fn unshare_userns(pid_cstr: &CStr, uid_map: &Map, gid_map: &Map) -> Result<()> {
    let (read, write) = pipe()?;
    match unsafe { fork() }? {
        ForkResult::Parent { child } => {
            unshare(CloneFlags::CLONE_NEWUSER)?;
            nix::unistd::close(read)?;
            nix::unistd::close(write)?;
            let status = waitpid(child, None)?;
            if status != WaitStatus::Exited(child, 0) {
                return Err(Errno::EIO);
            }
        }
        ForkResult::Child => {
            nix::unistd::close(write)?;
            nix::unistd::read(read, &mut [0u8])?;

            match unsafe { fork() } {
                Ok(ForkResult::Parent { child }) => {
                    let status = waitpid(child, None)?;
                    if status != WaitStatus::Exited(child, 0) {
                        return Err(Errno::EIO);
                    }
                    Ok(())
                }
                Ok(ForkResult::Child) => nix::unistd::execv(
                    CStr::from_bytes_with_nul(b"/usr/bin/newgidmap\0").expect("infallible"),
                    &[
                        CStr::from_bytes_with_nul(b"newgidmap\0").expect("infallible"),
                        pid_cstr,
                        CStr::from_bytes_with_nul(b"0\0").expect("infallible"),
                        gid_map.outside_root,
                        CStr::from_bytes_with_nul(b"1\0").expect("infallible"),
                        CStr::from_bytes_with_nul(b"1\0").expect("infallible"),
                        gid_map.outside_sub_start,
                        gid_map.len,
                    ],
                )
                .map(|_| ()),
                Err(e) => Err(e),
            }?;
            nix::unistd::execv(
                CStr::from_bytes_with_nul(b"/usr/bin/newuidmap\0").expect("infallible"),
                &[
                    CStr::from_bytes_with_nul(b"newuidmap\0").expect("infallible"),
                    pid_cstr,
                    CStr::from_bytes_with_nul(b"0\0").expect("infallible"),
                    uid_map.outside_root,
                    CStr::from_bytes_with_nul(b"1\0").expect("infallible"),
                    CStr::from_bytes_with_nul(b"1\0").expect("infallible"),
                    uid_map.outside_sub_start,
                    uid_map.len,
                ],
            )
            .expect("failed to exec newuidmap");
            unreachable!("we just exec-ed")
        }
    }
    Ok(())
}
