/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use slog::debug;
use slog::trace;
use slog::Logger;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio::time::timeout;
use zbus::Proxy;
use zbus::ProxyBuilder;

#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

pub mod analyze;
mod dbus_types;
mod escape;
mod machined_manager;
mod networkd_manager;
mod property_stream;
pub mod render;
mod system_state;
mod systemd_manager;
mod transient_unit;
pub use dbus_types::*;
pub use escape::*;
pub use machined_manager::ManagerProxy as MachinedManagerProxy;
pub use machined_manager::*;
pub use networkd_manager::ManagerProxy as NetworkdManagerProxy;
pub use networkd_manager::*;
pub use system_state::SystemState;
pub use system_state::WaitableSystemState;
pub use systemd_manager::ManagerProxy as SystemdManagerProxy;
pub use systemd_manager::*;
pub use transient_unit::Error as TransientUnitError;
pub use transient_unit::Opts as TransientUnitOpts;

pub static PROVIDER_ROOT: &str = "/usr/lib/systemd/system";

#[derive(Debug)]
pub struct ConnectOpts {
    connection_timeout: Duration,
    dbus_addr: String,
    retry_interval: Duration,
}

impl Default for ConnectOpts {
    fn default() -> Self {
        // very arbitrary default timeouts
        Self {
            connection_timeout: Duration::from_secs(2),
            dbus_addr: "unix:path=/run/dbus/system_bus_socket".to_owned(),
            retry_interval: Duration::from_millis(50),
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to connect to dbus")]
    Connect(zbus::Error),
    #[error("timed out connecting to dbus")]
    ConnectTimeout(tokio::time::error::Elapsed),
    #[error("{0:?}: {1}")]
    SystemState(SystemState, &'static str),
    #[error("error interacting with dbus")]
    Dbus(#[from] zbus::Error),
    #[error("error in property stream: {0}")]
    PropertyStream(String),
    #[error("transient unit failure: {0:?}")]
    TransientUnit(#[from] transient_unit::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

static DBUS_SYSTEM_BUS_ADDRESS_MUTEX: Lazy<Arc<Mutex<()>>> = Lazy::new(|| Arc::new(Mutex::new(())));

impl<T> DbusService<T>
where
    T: From<Proxy<'static>> + zbus::ProxyDefault,
{
    /// Establish a connection on the system dbus socket with the default
    /// configuration.
    pub async fn connect(log: Logger) -> Result<Self> {
        Self::connect_with_opts(log, ConnectOpts::default()).await
    }

    /// Establish a connection to the interface on the system dbus socket.
    /// There are many cases where this function will be called before
    /// dbus-daemon may be listening on the socket, so the connection will be
    /// retried for up to `opts.connect_timeout`, after which dbus-daemon should
    /// hopefully be up.
    pub async fn connect_with_opts(log: Logger, opts: ConnectOpts) -> Result<Self> {
        // The dbus library has no way of setting the system bus
        // address, so we have to override it in the environment. Use a
        // lock so that any concurrent connection attempts cannot step
        // over each other's path mangling.
        let _env_guard = DBUS_SYSTEM_BUS_ADDRESS_MUTEX.lock();
        // we unconditionally reset the DBUS_SYSTEM_BUS_ADDRESS env var on every
        // connection, so there is no need to save and restore the existing
        // value
        std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", &opts.dbus_addr);

        let mut last_err = None;
        let connection = timeout(opts.connection_timeout, async {
            loop {
                match zbus::Connection::system().await {
                    Ok(c) => return c,
                    Err(e) => {
                        last_err = Some(e);
                        sleep(opts.retry_interval).await;
                    }
                }
            }
        })
        .await
        .map_err(|elapsed| {
            last_err.map_or_else(|| Error::ConnectTimeout(elapsed), Error::Connect)
        })?;

        let proxy = ProxyBuilder::new(&connection)
            // we can't cache properties because systemd has some
            // properties that change but do not emit change signals
            .cache_properties(false)
            .build()
            .await
            .map_err(Error::Connect)?;

        Ok(Self { log, proxy })
    }

    pub fn logger(&self) -> Logger {
        // according to slog docs, cloning existing loggers and creating new
        // ones is cheap, so just make it easy
        self.log.clone()
    }
}

#[derive(Clone)]
pub struct DbusService<T> {
    log: Logger,
    proxy: T,
}

impl<T> std::ops::Deref for DbusService<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.proxy
    }
}

pub type Systemd = DbusService<SystemdManagerProxy<'static>>;

impl Systemd {
    /// Wait for systemd to enter some chosen "readiness" state. By the time
    /// this function returns, the system will be in either the requested state,
    /// or a state that follows it.
    pub async fn wait(&self, desired: WaitableSystemState) -> Result<()> {
        debug!(
            self.log,
            "waiting for system to enter at least {:?}", desired
        );

        // zbus has nice apis to get a futures stream of property change
        // updates, but of course systemd does not emit this event for the
        // SystemState property, so let's just poll for it
        let mut last_state: WaitableSystemState = self.system_state().await?.try_into()?;
        debug!(self.log, "system is in {:?}", last_state);
        loop {
            let state = self.system_state().await?.try_into()?;
            if state != last_state {
                trace!(
                    self.log,
                    "system changed from {:?} -> {:?}",
                    last_state,
                    state
                );
                last_state = state;
            }
            if state >= desired {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
}

pub type Machined = DbusService<MachinedManagerProxy<'static>>;
pub type Networkd = DbusService<NetworkdManagerProxy<'static>>;

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::Systemd;
    use super::WaitableSystemState;

    #[containertest]
    async fn test_dbus_api() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        let version = sd.version().await?;
        assert_ne!(version, "");
        Ok(())
    }

    #[containertest]
    async fn test_wait_for_startup() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        sd.wait(WaitableSystemState::Starting).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        Ok(())
    }
}
