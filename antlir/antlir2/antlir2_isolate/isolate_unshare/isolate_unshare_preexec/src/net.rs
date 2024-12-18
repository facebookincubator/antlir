/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::fd::AsRawFd;

use anyhow::Context;
use anyhow::Result;
use close_err::Closable;
use libc::c_char;
use libc::c_short;
use libc::ifreq;
use nix::errno::Errno;
use nix::sys::socket::AddressFamily;
use nix::sys::socket::SockFlag;
use nix::sys::socket::SockType;

/// Bring up the loopback interface named 'lo'. Does nothing but set the
/// interface to up.
pub(crate) fn bring_loopback_up() -> Result<()> {
    let socket = nix::sys::socket::socket(
        AddressFamily::Inet,
        SockType::Datagram,
        SockFlag::empty(),
        None,
    )
    .context("while opening socket")?;
    let mut req: ifreq = unsafe { std::mem::zeroed() };
    req.ifr_name[0] = 'l' as c_char;
    req.ifr_name[1] = 'o' as c_char;
    Errno::result(unsafe { libc::ioctl(socket.as_raw_fd(), libc::SIOCGIFFLAGS, &mut req) })
        .context("while getting existing flags")?;
    unsafe { req.ifr_ifru.ifru_flags |= libc::IFF_UP as c_short };
    Errno::result(unsafe { libc::ioctl(socket.as_raw_fd(), libc::SIOCSIFFLAGS, &mut req) })
        .context("while setting up flag")?;
    socket.close().context("while closing socket")?;
    Ok(())
}
