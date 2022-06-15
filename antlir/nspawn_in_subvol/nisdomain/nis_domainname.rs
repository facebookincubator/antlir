/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::CStr;
use std::os::raw::c_char;

use clap::{ArgEnum, Parser};
use nix::errno::Errno;

#[derive(ArgEnum, Clone, Debug)]
pub enum Op {
    Get,
    Set,
}

#[derive(Parser, Debug)]
struct Opts {
    #[clap(arg_enum, default_value_t = Op::Get)]
    operation: Op,
}

fn main() -> std::io::Result<()> {
    let opts = Opts::parse();
    match opts.operation {
        Op::Get => {
            let mut uname: nix::libc::utsname = unsafe { std::mem::zeroed() };
            let res = unsafe { nix::libc::uname(&mut uname) };
            match res {
                0 => {
                    let domainname_c =
                        unsafe { CStr::from_ptr(&uname.domainname as *const c_char) };
                    let domainname = domainname_c.to_str().unwrap();
                    println!("{}", domainname);
                    Ok(())
                }
                _ => Err(Errno::last().into()),
            }
        }
        Op::Set => {
            let hostname = b"AntlirNotABuildStep";
            let res =
                unsafe { nix::libc::setdomainname(hostname.as_ptr() as *const i8, hostname.len()) };
            match res {
                0 => Ok(()),
                _ => Err(Errno::last().into()),
            }
        }
    }
}
