/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsStr;
use std::hash::Hash;
use std::hash::Hasher;
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::ser::SerializeSeq;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use zvariant::OwnedValue;
use zvariant::Signature;
use zvariant::Type;

use crate::Result;

#[derive(Debug, Serialize)]
#[repr(transparent)]
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

impl FilePath {
    pub fn new<S: AsRef<OsStr> + ?Sized>(s: &S) -> &Self {
        unsafe { &*(s.as_ref() as *const OsStr as *const FilePath) }
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

impl From<OwnedFilePath> for PathBuf {
    fn from(o: OwnedFilePath) -> Self {
        o.0
    }
}

/// Systemd timestamp corresponding to CLOCK_REALTIME.
#[derive(Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
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

/// Unix signal that can be sent to a process
#[derive(Debug, PartialEq)]
pub struct Signal(nix::sys::signal::Signal);

impl Type for Signal {
    fn signature() -> Signature<'static> {
        i32::signature()
    }
}

impl std::ops::Deref for Signal {
    type Target = nix::sys::signal::Signal;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Signal {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i32(self.0 as i32)
    }
}

/// Map of environment variables that can be set for a process.
#[derive(Debug, PartialEq)]
pub struct Environment(HashMap<String, String>);

impl Type for Environment {
    fn signature() -> Signature<'static> {
        <&[&str]>::signature()
    }
}

impl std::ops::Deref for Environment {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<Vec<String>> for Environment {
    type Error = String;

    fn try_from(v: Vec<String>) -> std::result::Result<Self, Self::Error> {
        v.into_iter()
            .map(|e| {
                e.split_once("=")
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .ok_or_else(|| format!("invalid env pair {}", e))
            })
            .collect::<std::result::Result<_, String>>()
            .map(Self)
    }
}

impl Serialize for Environment {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for (key, val) in self.0.iter() {
            seq.serialize_element(&format!("{}={}", key, val))?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).and_then(|environs: Vec<String>| {
            environs.try_into().map_err(::serde::de::Error::custom)
        })
    }
}

impl TryFrom<OwnedValue> for Environment {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into()
            .and_then(|environs: Vec<String>| environs.try_into().map_err(zvariant::Error::Message))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uuid(uuid::Uuid);

impl Type for Uuid {
    fn signature() -> Signature<'static> {
        <Vec<u8> as Type>::signature()
    }
}

impl TryFrom<OwnedValue> for Uuid {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into()
            .and_then(|u: Vec<u8>| {
                uuid::Uuid::from_slice(&u).map_err(|e| zvariant::Error::Message(format!("{:?}", e)))
            })
            .map(Self)
    }
}

/// [zbus]'s default object path types have no notion of the type of proxy they
/// are meant to represent. This is mostly fine for methods that get a single
/// object path back, as zbus can convert those responses, but objects will
/// frequently reference other objects by path, or methods will return lists of
/// paths, and we should have a safe way to load Proxys from them.
#[derive(Debug, Clone)]
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

impl<T> PartialEq<zvariant::OwnedObjectPath> for TypedObjectPath<T> {
    fn eq(&self, other: &zvariant::OwnedObjectPath) -> bool {
        &self.0 == other
    }
}

impl<T> Eq for TypedObjectPath<T> {}

impl<T> Hash for TypedObjectPath<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> Deref for TypedObjectPath<T> {
    type Target = zvariant::OwnedObjectPath;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

// Conversion from a typed to an untyped object path is always safe, and is
// necessary for certain dbus calls that expect object paths
impl<T> From<TypedObjectPath<T>> for zvariant::OwnedObjectPath {
    fn from(t: TypedObjectPath<T>) -> Self {
        t.0
    }
}

impl<T> TypedObjectPath<T> {
    /// Convert an untyped dbus object path to a type-safe version. This is
    /// inherently unsafe as it cannot be confirmed what type of object the path
    /// actually points to, but it can be reasonably guaranteed by the caller if
    /// they understand the DBus api being called.
    pub unsafe fn from_untyped<'a>(o: &zvariant::ObjectPath<'a>) -> Self {
        Self(o.clone().into(), PhantomData)
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
    use super::Environment;
    use super::MonotonicTimestamp;
    use super::Timestamp;
    use crate::Systemd;
    use anyhow::Result;
    use byteorder::LE;
    use maplit::hashmap;
    use maplit::hashset;
    use std::collections::HashSet;
    use std::time::Duration;
    use std::time::UNIX_EPOCH;
    use zvariant::from_slice;
    use zvariant::to_bytes;
    use zvariant::EncodingContext as Context;

    #[containertest]
    async fn test_typed_path() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        let units = sd.list_units().await?;
        assert!(!units.is_empty());
        let root = units.iter().find(|u| u.name == "-.mount").unwrap();
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

    #[test]
    fn test_environment() -> Result<()> {
        let ctxt = Context::<LE>::new_dbus(0);

        let encoded = to_bytes(ctxt, &vec!["HELLO=world", "FOO=bar"])?;
        let decoded: Environment = from_slice(&encoded, ctxt)?;
        assert_eq!(
            *decoded,
            hashmap! {"HELLO".into() => "world".into(), "FOO".into() => "bar".into()}
        );

        let encoded = to_bytes(ctxt, &decoded)?;
        let decoded: Vec<String> = from_slice(&encoded, ctxt)?;
        assert_eq!(
            HashSet::from_iter(decoded.into_iter()),
            hashset!["FOO=bar".into(), "HELLO=world".into()]
        );
        Ok(())
    }
}
