/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This is a helper library for unsharing the current process into a new,
//! unprivileged user namespace.
//! See the C implementation for more details about how exactly it works, but
//! the useful end result is that the process that calls this function will end
//! up in a new user namespace with a full range of UIDs and GIDs mapped into
//! it.

use std::ffi::CStr;
use std::io::Error;

#[derive(Copy, Clone)]
pub struct Map<'a> {
    pub outside_root: &'a CStr,
    pub outside_sub_start: &'a CStr,
    pub len: &'a CStr,
}

mod c {
    use std::os::raw::c_char;
    unsafe extern "C" {
        pub(crate) fn unshare_userns(
            pid_cstr: *const c_char,
            uid_map_outside_root: *const c_char,
            uid_map_outside_sub_start: *const c_char,
            uid_map_len: *const c_char,
            gid_map_outside_root: *const c_char,
            gid_map_outside_sub_start: *const c_char,
            gid_map_len: *const c_char,
        ) -> i32;
    }
}

pub fn unshare_userns(pid_cstr: &CStr, uid_map: &Map, gid_map: &Map) -> std::io::Result<()> {
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
    let res = unsafe {
        c::unshare_userns(
            pid_cstr.as_ptr(),
            uid_map.outside_root.as_ptr(),
            uid_map.outside_sub_start.as_ptr(),
            uid_map.len.as_ptr(),
            gid_map.outside_root.as_ptr(),
            gid_map.outside_sub_start.as_ptr(),
            gid_map.len.as_ptr(),
        )
    };
    match res {
        0 => Ok(()),
        -1 => Err(Error::last_os_error()),
        _ => Err(Error::from_raw_os_error(res)),
    }
}
