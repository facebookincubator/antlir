/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::CStr;
use std::ffi::CString;
use std::fmt::Debug;
use std::fmt::Display;
use std::os::fd::AsRawFd;
use std::os::raw::c_char;
use std::os::raw::c_void;
use std::str::FromStr;

use libc::ENODATA;

pub type Result<T> = std::io::Result<T>;

pub struct Capabilities(libcap_sys::cap_t);

pub trait FileExt {
    fn get_capabilities(&self) -> Result<Option<Capabilities>>;
    fn set_capabilities(&self, capabilities: Option<&Capabilities>) -> Result<()>;
}

impl FileExt for std::fs::File {
    fn get_capabilities(&self) -> Result<Option<Capabilities>> {
        let ret = unsafe { libcap_sys::cap_get_fd(self.as_raw_fd()) };
        if ret.is_null() {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error().expect("must be set") == ENODATA {
                Ok(None)
            } else {
                Err(err)
            }
        } else {
            Ok(Some(Capabilities(ret)))
        }
    }

    fn set_capabilities(&self, capabilities: Option<&Capabilities>) -> Result<()> {
        let ret = unsafe {
            libcap_sys::cap_set_fd(
                self.as_raw_fd(),
                capabilities.map_or(std::ptr::null_mut(), |c| c.0),
            )
        };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl Drop for Capabilities {
    fn drop(&mut self) {
        unsafe {
            libcap_sys::cap_free(self.0 as *mut c_void);
        }
    }
}

struct CapText(*mut c_char);

impl Drop for CapText {
    fn drop(&mut self) {
        unsafe {
            libcap_sys::cap_free(self.0 as *mut c_void);
        }
    }
}

impl Capabilities {
    fn cap_text(&self) -> Result<CapText> {
        let s = unsafe { libcap_sys::cap_to_text(self.0, std::ptr::null_mut()) };
        if s.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(CapText(s))
        }
    }

    pub fn to_text(&self) -> Result<String> {
        let cap_text = self.cap_text()?;
        let cstr = unsafe { CStr::from_ptr(cap_text.0) };
        cstr.to_str()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            .map(|s| s.to_owned())
    }
}

impl FromStr for Capabilities {
    type Err = std::io::Error;

    fn from_str(s: &str) -> Result<Self> {
        let cstr = CString::new(s).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "string had interior NUL byte")
        })?;
        let cap = unsafe { libcap_sys::cap_from_text(cstr.as_ptr()) };
        if cap.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Capabilities(cap))
        }
    }
}

impl Clone for Capabilities {
    fn clone(&self) -> Self {
        Self(unsafe { libcap_sys::cap_dup(self.0) })
    }
}

impl Debug for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Capabilities")
            .field(&self.to_text().map_err(|_| std::fmt::Error)?)
            .finish()
    }
}

impl Display for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_text().map_err(|_| std::fmt::Error)?)
    }
}

impl PartialEq for Capabilities {
    fn eq(&self, other: &Self) -> bool {
        let cmp = unsafe { libcap_sys::cap_compare(self.0, other.0) };
        cmp == 0
    }
}

impl Eq for Capabilities {}
