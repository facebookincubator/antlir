/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nix::unistd::Gid;
use nix::unistd::Uid;
use once_cell::sync::OnceCell;
use tracing::error;
use tracing::trace;

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
