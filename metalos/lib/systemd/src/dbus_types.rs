/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::convert::{TryFrom, TryInto};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize};
use zvariant::{OwnedValue, Signature, Type};

use crate::Result;

#[derive(Debug, Serialize)]
pub struct FilePath(Path);

impl Type for FilePath {
    fn signature() -> Signature<'static> {
        String::signature()
    }
}

impl std::ops::Deref for FilePath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OwnedFilePath(PathBuf);

impl Type for OwnedFilePath {
    fn signature() -> Signature<'static> {
        String::signature()
    }
}

impl std::ops::Deref for OwnedFilePath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<OwnedValue> for OwnedFilePath {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into().map(|s: String| OwnedFilePath(s.into()))
    }
}

/// Systemd timestamp corresponding to CLOCK_REALTIME.
#[derive(Debug, PartialEq)]
pub struct Timestamp(SystemTime);

impl Type for Timestamp {
    fn signature() -> Signature<'static> {
        u64::signature()
    }
}

impl std::ops::Deref for Timestamp {
    type Target = SystemTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<OwnedValue> for Timestamp {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into()
            .map(|t: u64| Self(UNIX_EPOCH + Duration::from_secs(t)))
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|t| Self(UNIX_EPOCH + Duration::from_secs(t)))
    }
}

/// Systemd timestamp corresponding to CLOCK_MONOTONIC.
#[derive(Debug, PartialEq)]
pub struct MonotonicTimestamp(Duration);

impl Type for MonotonicTimestamp {
    fn signature() -> Signature<'static> {
        u64::signature()
    }
}

impl std::ops::Deref for MonotonicTimestamp {
    type Target = Duration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<OwnedValue> for MonotonicTimestamp {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into().map(|t: u64| Self(Duration::from_secs(t)))
    }
}

impl<'de> Deserialize<'de> for MonotonicTimestamp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|t| Self(Duration::from_secs(t)))
    }
}

/// [zbus]'s default object path types have no notion of the type of proxy they
/// are meant to represent. This is mostly fine for methods that get a single
/// object path back, as zbus can convert those responses, but objects will
/// frequently reference other objects by path, or methods will return lists of
/// paths, and we should have a safe way to load Proxys from them.
#[derive(Debug)]
pub struct TypedObjectPath<T>(zvariant::OwnedObjectPath, PhantomData<T>);

impl<'de, T> Deserialize<'de> for TypedObjectPath<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|p| TypedObjectPath(p, PhantomData))
    }
}

impl<T> PartialEq for TypedObjectPath<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for TypedObjectPath<T> {}

impl<T> Type for TypedObjectPath<T> {
    fn signature() -> Signature<'static> {
        zvariant::OwnedObjectPath::signature()
    }
}

impl<T> TryFrom<OwnedValue> for TypedObjectPath<T> {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into()
            .map(|p: zvariant::OwnedObjectPath| TypedObjectPath(p, PhantomData))
    }
}

impl<T> TypedObjectPath<T>
where
    T: From<zbus::Proxy<'static>> + zbus::ProxyDefault,
{
    /// Load an object of the specified type from this path, using an existing
    /// connection.
    pub async fn load(&self, conn: &zbus::Connection) -> Result<T> {
        Ok(zbus::ProxyBuilder::new(conn)
            // This can only fail if the input cannot be converted to a path. In
            // this case it obviously is already a path... what a dumb api
            .path(self.0.clone())?
            // we can't cache properties because systemd has some
            // properties that change but do not emit change signals
            .cache_properties(false)
            .build()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::{MonotonicTimestamp, Timestamp};
    use crate::Systemd;
    use anyhow::Result;
    use byteorder::LE;
    use std::time::{Duration, UNIX_EPOCH};
    use zvariant::EncodingContext as Context;
    use zvariant::{from_slice, to_bytes};

    #[containertest]
    async fn test_typed_path() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        let units = sd.list_units().await?;
        assert!(units.len() > 0);
        let root = units.iter().find(|u| u.name == "-.mount".into()).unwrap();
        let unit = root.unit.load(sd.connection()).await?;
        assert_eq!(unit.id().await?, root.name);
        Ok(())
    }

    #[test]
    fn test_timestamps() -> Result<()> {
        let ctxt = Context::<LE>::new_dbus(0);

        let encoded = to_bytes(ctxt, &100u64)?;
        let decoded: Timestamp = from_slice(&encoded, ctxt)?;
        assert_eq!(*decoded, UNIX_EPOCH + Duration::from_secs(100));

        let encoded = to_bytes(ctxt, &100u64)?;
        let decoded: MonotonicTimestamp = from_slice(&encoded, ctxt)?;
        assert_eq!(*decoded, Duration::from_secs(100));
        Ok(())
    }
}
