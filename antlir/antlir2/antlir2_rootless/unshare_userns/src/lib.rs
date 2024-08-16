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
    // TODO(T181212521): do the same check in OSS
    #[cfg(facebook)]
    if memory::is_using_jemalloc()
        && memory::mallctl_read::<bool>("background_thread").expect("Err reading mallctl")
    {
        panic!(
            "jemalloc background thread is enabled!\nThis is incompatible with unshare_userns, \
             please check your binary's `malloc_conf` or set the binary target's `allocator` attribute to \"malloc\"."
        );
    }
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
                    c"/usr/bin/newgidmap",
                    &[
                        c"newgidmap",
                        pid_cstr,
                        c"0",
                        gid_map.outside_root,
                        c"1",
                        c"1",
                        gid_map.outside_sub_start,
                        gid_map.len,
                    ],
                )
                .map(|_| ()),
                Err(e) => Err(e),
            }?;
            nix::unistd::execv(
                c"/usr/bin/newuidmap",
                &[
                    c"newuidmap",
                    pid_cstr,
                    c"0",
                    uid_map.outside_root,
                    c"1",
                    c"1",
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
