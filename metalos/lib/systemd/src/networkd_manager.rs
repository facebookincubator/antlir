/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use zbus::dbus_proxy;
use zvariant::derive::Type;

use crate::dbus_types::*;
use systemd_macros::SystemdEnum;
use systemd_macros::TransparentZvariant;

#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct LinkIndex(i32);

#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct LinkName(String);

#[derive(Debug, PartialEq, Eq, Deserialize, Type)]
pub struct ListedLink {
    pub index: LinkIndex,
    pub name: LinkName,
    pub path: TypedObjectPath<LinkProxy<'static>>,
}

#[dbus_proxy(
    interface = "org.freedesktop.network1.Manager",
    default_service = "org.freedesktop.network1",
    default_path = "/org/freedesktop/network1",
    gen_blocking = false
)]
trait Manager {
    /// List all links on the system. However do note that some or all of them
    /// may not be managed by systemd-networkd - see
    /// [LinkProxy::administrative_state] to determine if it is managed or not.
    fn list_links(&self) -> zbus::Result<Vec<ListedLink>>;

    /// Lookup a link by its interface name.
    fn get_link_by_name(
        &self,
        name: LinkName,
    ) -> zbus::Result<(LinkIndex, TypedObjectPath<LinkProxy<'static>>)>;

    /// Lookup a link by its interface index.
    fn get_link_by_index(
        &self,
        idx: LinkIndex,
    ) -> zbus::Result<(LinkName, TypedObjectPath<LinkProxy<'static>>)>;
}

#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum AdministrativeState {
    Managed,
    Unmanaged,
    Unknown(String),
}

#[dbus_proxy(
    interface = "org.freedesktop.network1.Link",
    default_service = "org.freedesktop.network1",
    gen_blocking = false
)]
trait Link {
    #[dbus_proxy(property)]
    fn administrative_state(&self) -> zbus::Result<AdministrativeState>;
}

#[cfg(test)]
mod tests {
    use super::AdministrativeState;
    use crate::Networkd;
    use anyhow::Result;

    #[containertest]
    async fn test_network_api() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let nd = Networkd::connect(log.clone()).await?;
        let links = nd.list_links().await?;
        assert!(!links.is_empty());

        let (lo_idx, lo_path) = nd.get_link_by_name("lo".into()).await?;
        let lo = lo_path.load(nd.connection()).await?;
        assert_eq!(
            AdministrativeState::Unmanaged,
            lo.administrative_state().await?
        );

        let lo_name = nd.get_link_by_index(lo_idx).await?.0;
        assert_eq!("lo", lo_name);
        Ok(())
    }
}
