/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::CString;

use antlir2_userns::subid::IdMap;
use nix::unistd::Gid;
use nix::unistd::Uid;
use once_cell::sync::OnceCell;
use tracing::error;
use tracing::trace;
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to parse id: {0}")]
    IdParse(#[from] std::num::ParseIntError),
    #[error("failed to set uid to {id}: {error}")]
    SetUid { id: Uid, error: std::io::Error },
    #[error("failed to set gid to {id}: {error}")]
    SetGid { id: Gid, error: std::io::Error },
    #[error("Rootless was somehow already initialized")]
    AlreadyInitialized,
    #[error("failed to setup userns in current process: {0}")]
    Userns(nix::errno::Errno),
    #[error("error reading a subid file: {0}")]
    SubidRead(std::io::Error),
    #[error("error using subid mapping: {0}")]
    Subid(antlir2_userns::subid::Error),
    #[error("error getting username of {0}: {1}")]
    GetUsername(nix::unistd::Uid, nix::errno::Errno),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Copy, Clone)]
pub struct Rootless {
    setuid: Option<Uid>,
    setgid: Option<Gid>,
}

static INSTANCE: OnceCell<Rootless> = OnceCell::new();

impl Rootless {
    pub fn init() -> Result<Self> {
        if let Some(i) = INSTANCE.get() {
            Ok(*i)
        } else {
            // If this process is not root any sete*id calls will fail, but it
            // doesn't matter since we can assume that we're already the correct
            // user for this build environment.
            if !Uid::effective().is_root() {
                // SUDO_*ID env vars may have leaked from an outer `sudo`
                // wrapper at some level so cannot be trusted
                trace!("euid is not root, not checking SUDO_*ID env vars");
                return Self::init_with_ids(None, None);
            }
            let setuid = if let Ok(uid_str) = std::env::var("SUDO_UID") {
                Some(Uid::from_raw(uid_str.parse()?))
            } else {
                None
            };
            let setgid = if let Ok(gid_str) = std::env::var("SUDO_GID") {
                Some(Gid::from_raw(gid_str.parse()?))
            } else {
                None
            };
            Self::init_with_ids(setuid, setgid)
        }
    }

    pub fn init_with_ids(setuid: Option<Uid>, setgid: Option<Gid>) -> Result<Self> {
        if let Some(i) = INSTANCE.get() {
            return Ok(*i);
        }
        if let Some(setgid) = setgid {
            trace!("setegid({})", setgid);
            nix::unistd::setegid(setgid).map_err(|e| Error::SetGid {
                id: setgid,
                error: e.into(),
            })?;
        }
        if let Some(setuid) = setuid {
            trace!("seteuid({})", setuid);
            nix::unistd::seteuid(setuid).map_err(|e| Error::SetUid {
                id: setuid,
                error: e.into(),
            })?;
        }
        let s = Self { setuid, setgid };
        INSTANCE.set(s).map_err(|_| Error::AlreadyInitialized)?;
        Ok(s)
    }

    pub fn as_root<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce() -> R,
    {
        let _token = self.escalate()?;
        Ok(f())
    }

    /// Escalate to root privileges for as long as this [EscalationGuard] is in
    /// scope.
    pub fn escalate(&self) -> Result<EscalationGuard> {
        trace!("escalating privileges to root");
        nix::unistd::setegid(Gid::from_raw(0)).map_err(|e| Error::SetGid {
            id: Gid::from_raw(0),
            error: e.into(),
        })?;
        nix::unistd::seteuid(Uid::from_raw(0)).map_err(|e| Error::SetUid {
            id: Uid::from_raw(0),
            error: e.into(),
        })?;
        Ok(EscalationGuard {
            setuid: self.setuid,
            setgid: self.setgid,
        })
    }
}

/// As long as this [EscalationGuard] is in scope, the process will be running
/// as root.
#[must_use]
pub struct EscalationGuard {
    setuid: Option<Uid>,
    setgid: Option<Gid>,
}

impl Drop for EscalationGuard {
    fn drop(&mut self) {
        trace!("dropping privileges");
        if let Some(setgid) = self.setgid {
            trace!("setegid({})", setgid);
            if let Err(e) = nix::unistd::setegid(setgid).map_err(|e| Error::SetGid {
                id: setgid,
                error: e.into(),
            }) {
                error!("{}", e.to_string());
            }
        }
        if let Some(setuid) = self.setuid {
            trace!("seteuid({})", setuid);
            if let Err(e) = nix::unistd::seteuid(setuid).map_err(|e| Error::SetUid {
                id: setuid,
                error: e.into(),
            }) {
                error!("{}", e.to_string());
            }
        }
    }
}

pub fn init() -> Result<Rootless> {
    Rootless::init()
}

#[tracing::instrument]
pub fn unshare_new_userns() -> Result<()> {
    let current_uid = Uid::current();

    if current_uid.is_root() {
        warn!("running as root, not using a user namespace");
        return Ok(());
    }

    let current_name = nix::unistd::User::from_uid(current_uid)
        .map_err(|e| Error::GetUsername(current_uid, e))?
        .ok_or(Error::GetUsername(current_uid, nix::errno::Errno::ENOENT))?
        .name;
    trace!("looking up best subid ranges for {current_uid} ({current_name})");

    let uid_map: IdMap<antlir2_userns::Uid> = std::fs::read_to_string("/etc/subuid")
        .map_err(Error::SubidRead)?
        .parse()
        .map_err(Error::Subid)?;
    let gid_map: IdMap<antlir2_userns::Gid> = std::fs::read_to_string("/etc/subgid")
        .map_err(Error::SubidRead)?
        .parse()
        .map_err(Error::Subid)?;
    let uid_range = uid_map
        .best(&antlir2_userns::subid::User::Id(
            current_uid.as_raw().into(),
        ))
        .or_else(|| uid_map.best(&antlir2_userns::subid::User::Name(current_name.clone())))
        .ok_or_else(|| {
            Error::Subid(antlir2_userns::subid::Error::msg(format!(
                "no subid range for {}/{}",
                current_uid, current_name
            )))
        })?;
    let gid_range = gid_map
        .best(&antlir2_userns::subid::User::Id(
            current_uid.as_raw().into(),
        ))
        .or_else(|| gid_map.best(&antlir2_userns::subid::User::Name(current_name.clone())))
        .ok_or_else(|| Error::Subid(antlir2_userns::subid::Error::msg("no subid range")))?;

    trace!("using uid range {uid_range:?} and gid range {gid_range:?}");

    let pid_cstring =
        CString::new(std::process::id().to_string()).expect("never has an internal null byte");

    let outside_uid_cstr = CString::new(current_uid.to_string()).expect("infallible");
    let outside_uid_sub_start_cstr =
        CString::new(uid_range.start().to_string()).expect("infallible");
    let outside_uid_len_cstr = CString::new(uid_range.len().to_string()).expect("infallible");

    let uid_map = unshare_userns::Map {
        outside_root: &outside_uid_cstr,
        outside_sub_start: &outside_uid_sub_start_cstr,
        len: &outside_uid_len_cstr,
    };

    let outside_gid_cstr = CString::new(Gid::current().to_string()).expect("infallible");
    let outside_gid_sub_start_cstr =
        CString::new(gid_range.start().to_string()).expect("infallible");
    let outside_gid_len_cstr = CString::new(gid_range.len().to_string()).expect("infallible");

    let gid_map = unshare_userns::Map {
        outside_root: &outside_gid_cstr,
        outside_sub_start: &outside_gid_sub_start_cstr,
        len: &outside_gid_len_cstr,
    };

    unshare_userns::unshare_userns(&pid_cstring, &uid_map, &gid_map).map_err(Error::Userns)
}
