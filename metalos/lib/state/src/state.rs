/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(trait_alias)]

//! MetalOS manages state in various subvolumes, this crate is a common api for
//! managing that state on disk. MetalOS code should use the functionality of
//! this crate instead of directly dealing with the filesystem (or any other
//! backing store), so that we can avoid a proliferation of hardcoded paths or
//! unrelated implementation details.
//! Additionally, having this in a separate crate makes it trivial to swap out
//! the filesystem for something like a proper database, if that ever becomes
//! necessary.

use std::fmt::Debug;
use std::marker::PhantomData;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use anyhow::{ensure, Context, Error, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use uuid::Uuid;

static STATE_BASE: Lazy<PathBuf> = Lazy::new(|| {
    #[cfg(not(test))]
    {
        metalos_paths::metalos_state().into()
    }
    #[cfg(test)]
    {
        tempfile::tempdir().unwrap().into_path()
    }
});

/// Special UUID for the "staged" value which holds special meaning
/// c4dbff2e-2c0a-4e35-85a2-2ccb7dd8d8a9
static UUID_STAGED: Uuid = Uuid::from_u128(261670975858436844194139735623489607849);
/// Special UUID for the "current" value which holds special meaning
/// 80e27f0c-75dd-4855-9ada-addea2d90a1b
static UUID_CURRENT: Uuid = Uuid::from_u128(171317219403732977053801729476395600411);

mod __private {
    pub trait Sealed {}
}

trait SerdeState = serde::de::DeserializeOwned + serde::Serialize;

trait ThriftState = fbthrift::Serialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolSerializer<bufsize::SizeCounter>,
    > + fbthrift::Serialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolSerializer<bytes::BytesMut>,
    > + fbthrift::Deserialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolDeserializer<std::io::Cursor<Bytes>>,
    >;

/// Abstraction on different serializers (thrift and serde) so that this library
/// can operate with types that are serializable with either Thrift or Serde.
pub trait Serialization: __private::Sealed {}

pub struct Serde;

impl __private::Sealed for Serde {}
impl Serialization for Serde {}

pub struct Thrift;

impl __private::Sealed for Thrift {}
impl Serialization for Thrift {}

/// Any type that can be serialized to disk and loaded later with then unique id.
pub trait State<Ser>: Sized + Debug
where
    Ser: Serialization,
{
    fn to_json(&self) -> Result<Vec<u8>>;
    fn from_json(bytes: Vec<u8>) -> Result<Self>;
}

impl<T> State<Serde> for T
where
    T: Sized + Debug + SerdeState,
{
    fn to_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(Error::msg)
    }
    fn from_json(bytes: Vec<u8>) -> Result<Self> {
        serde_json::from_slice(&bytes).map_err(Error::msg)
    }
}

impl<T> State<Thrift> for T
where
    T: Sized + Debug + ThriftState,
{
    fn to_json(&self) -> Result<Vec<u8>> {
        Ok(fbthrift::simplejson_protocol::serialize(self).to_vec())
    }
    fn from_json(bytes: Vec<u8>) -> Result<Self> {
        fbthrift::simplejson_protocol::deserialize(bytes)
    }
}

/// Unique reference to a piece of state of a specific type. Can be used to
/// retrieve the state from disk via [load]
pub struct Token<S, Ser = Serde>(Uuid, PhantomData<(S, Ser)>)
where
    S: State<Ser>,
    Ser: Serialization;

impl<S, Ser> Clone for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    fn clone(&self) -> Self {
        Token::new(self.0)
    }
}

impl<S, Ser> Copy for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
}

impl<S, Ser> PartialEq for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S, Ser> Eq for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
}

impl<S, Ser> std::fmt::Debug for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("type", &std::any::type_name::<S>())
            .field("token", &self.0)
            .finish()
    }
}

impl<S, Ser> std::fmt::Display for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", &std::any::type_name::<S>(), self.0.to_simple())
    }
}

unsafe impl<S, Ser> Send for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
}
unsafe impl<S, Ser> Sync for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
}

impl<S, Ser> std::str::FromStr for Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let (ty, uuid) = s
            .rsplit_once("::")
            .with_context(|| format!("'{}' missing '::' separator", s))?;
        ensure!(
            ty == std::any::type_name::<S>(),
            "expected type '{}', got '{}'",
            std::any::type_name::<S>(),
            ty
        );
        let uuid: Uuid = uuid
            .parse()
            .with_context(|| format!("'{}' is not a valid uuid", uuid))?;
        Ok(Self(uuid, PhantomData))
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
/// There are a few special cased tokens that hold meaning in MetalOS.
pub enum Alias {
    /// The most recently staged version of a state variable.
    Staged,
    /// The most recently committed version of a state variable.
    Current,
}

impl Alias {
    fn token<S, Ser>(self) -> Token<S, Ser>
    where
        S: State<Ser>,
        Ser: Serialization,
    {
        Token::new(match self {
            Self::Current => UUID_CURRENT,
            Self::Staged => UUID_STAGED,
        })
    }
}

impl<S, Ser> Token<S, Ser>
where
    S: State<Ser>,
    Ser: Serialization,
{
    fn new(uuid: Uuid) -> Self {
        Self(uuid, PhantomData)
    }

    fn path(&self) -> PathBuf {
        // the type name is used to provide somewhat human-readable information
        // about the files on disk (eg if someone runs `ls`)
        let filename = format!("{}-{}.json", std::any::type_name::<S>(), &self.0);
        STATE_BASE.join(filename)
    }

    /// Token pointing to the most recently staged state item. This may or may
    /// not exist on disk.
    pub fn staged() -> Self {
        Alias::Staged.token()
    }

    /// Token pointing to the most recently committed state item. This may or
    /// may not exist on disk.
    pub fn current() -> Self {
        Alias::Current.token()
    }

    /// Mark this token as the staged version of a state item.
    ///
    /// See also [commit](Token::commit).
    pub fn stage(&self) -> Result<()> {
        alias(*self, Alias::Staged)
    }

    /// Mark this token as the current version of a state item.
    ///
    /// Typically precededed by [stage](Token::stage), but this is not required.
    /// [stage](Token::stage) and [commit](Token::commit) hold special meaning
    /// and can be used to retrieve states without knowing the unique [Token].
    pub fn commit(&self) -> Result<()> {
        alias(*self, Alias::Current)
    }
}

/// Persist a new version of a state type, getting back a unique key to later
/// load it with.
pub fn save<S, Ser>(state: &S) -> Result<Token<S, Ser>>
where
    S: State<Ser>,
    Ser: Serialization,
{
    save_with_uuid(state, Uuid::new_v4())
}

/// Persist a new version of a state type with a preset UUID.
pub fn save_with_uuid<S, Ser>(state: &S, uuid: Uuid) -> Result<Token<S, Ser>>
where
    S: State<Ser>,
    Ser: Serialization,
{
    let token = Token::new(uuid);
    let p = token.path();
    let state = state
        .to_json()
        .with_context(|| format!("while serializing {:?}", state))?;
    std::fs::write(&p, &state).with_context(|| format!("while serializing to {}", p.display()))?;
    Ok(token)
}

/// Save this specific token as a special [Alias]. If this alias already exists,
/// it will be replaced.
fn alias<S, Ser>(token: Token<S, Ser>, alias: Alias) -> Result<()>
where
    S: State<Ser>,
    Ser: Serialization,
{
    let alias: Token<S, Ser> = alias.token();
    let alias_path = alias.path();
    std::fs::remove_file(&alias_path)
        .or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
        .with_context(|| format!("while removing existing alias {}", alias_path.display()))?;
    symlink(token.path(), alias.path()).with_context(|| {
        format!(
            "while symlinking alias {} -> {}",
            alias_path.display(),
            token.path().display()
        )
    })
}

/// Load a specific version of a state type, using the key returned by [save]
pub fn load<S, Ser>(token: Token<S, Ser>) -> Result<Option<S>>
where
    S: State<Ser>,
    Ser: Serialization,
{
    match std::fs::read(token.path()) {
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => return Ok(None),
                _ => Err(anyhow::Error::from(e)
                    .context(format!("while opening {}", token.path().display()))),
            }
        }
        Ok(bytes) => S::from_json(bytes)
            .map(Some)
            .with_context(|| format!("while deserializing {}", token.path().display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use serde::{Deserialize, Serialize};
    use std::ops::Deref;

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct ExampleState {
        hello: String,
    }

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct Other {
        goodbye: String,
    }

    #[test]
    fn parse() -> Result<()> {
        assert_eq!(
            Token::new("0e2d4f4a-b09b-4a55-b6fd-fd57a60b9de8".parse().unwrap()),
            "state::tests::ExampleState::0e2d4f4a-b09b-4a55-b6fd-fd57a60b9de8"
                .parse::<Token<ExampleState>>()
                .unwrap()
        );
        assert_eq!(
            "expected type 'state::tests::Other', got 'state::tests::ExampleState'",
            "state::tests::ExampleState::0e2d4f4a-b09b-4a55-b6fd-fd57a60b9de8"
                .parse::<Token<Other>>()
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            "'not-a-uuid' is not a valid uuid",
            "state::tests::ExampleState::not-a-uuid"
                .parse::<Token<ExampleState>>()
                .unwrap_err()
                .to_string()
        );
        Ok(())
    }

    #[test]
    fn current() -> Result<()> {
        std::fs::create_dir_all(STATE_BASE.deref())?;
        let current: Option<ExampleState> =
            load(Token::current()).context("while loading non-existent current")?;
        assert_eq!(None, current);
        let token = save(&ExampleState {
            hello: "world".into(),
        })
        .context("while saving")?;
        alias(token, Alias::Current).context("while writing current alias")?;
        let current_token = Token::current();
        let current = load(current_token).context("while loading current")?;
        assert_eq!(
            Some(ExampleState {
                hello: "world".into()
            }),
            current
        );
        Ok(())
    }

    fn kv_test<Ser: Serialization, T: State<Ser> + PartialEq>(t: T) -> Result<()> {
        std::fs::create_dir_all(STATE_BASE.deref())?;
        let token = save(&t).context("while saving")?;
        let loaded = load(token).context("while loading")?;
        assert_eq!(Some(t), loaded);
        Ok(())
    }

    #[test]
    fn kv_serde() -> Result<()> {
        kv_test(ExampleState {
            hello: "world".into(),
        })
    }

    #[test]
    fn kv_thrift() -> Result<()> {
        kv_test(example::Example {
            hello: "world".into(),
        })
    }

    #[test]
    fn kv_thrift_and_serde() -> Result<()> {
        // this thrift struct comes with multiple possible implementations, and
        // the compiler cannot choose between them automatically
        kv_test::<Thrift, _>(example_with_serde::Example {
            hello: "world".into(),
        })?;
        kv_test::<Serde, _>(example_with_serde::Example {
            hello: "world".into(),
        })
    }
}
