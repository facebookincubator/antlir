/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;

use crate::Operation;

pub(super) fn xattr_ops<C, F>(old: Option<F>, new: F) -> std::io::Result<Vec<Operation<C>>>
where
    F: AsRawFd,
{
    let new = get_xattrs(new.as_raw_fd())?;
    let old = old
        .map(|o| get_xattrs(o.as_raw_fd()))
        .transpose()?
        .unwrap_or_default();

    let mut ops = Vec::new();

    for old_name in old.keys() {
        if !new.contains_key(old_name) {
            ops.push(Operation::RemoveXattr {
                name: old_name.clone(),
            });
        }
    }
    for (name, value) in new {
        let changed = match old.get(&name) {
            Some(old_value) => old_value != &value,
            None => true,
        };
        if changed {
            ops.push(Operation::SetXattr { name, value });
        }
    }

    Ok(ops)
}

fn get_xattrs(f: RawFd) -> std::io::Result<BTreeMap<OsString, Vec<u8>>> {
    let path = std::fs::read_link(format!("/proc/self/fd/{f}"))?;
    let mut xattrs = BTreeMap::new();
    for name in xattr::list(&path)? {
        if let Some(value) = xattr::get(&path, &name)? {
            xattrs.insert(name, value);
        }
    }
    Ok(xattrs)
}
