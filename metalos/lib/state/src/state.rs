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

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use once_cell::sync::Lazy;
use sha2::Digest;
use sha2::Sha256;
use url::Url;

type Sha256Value = [u8; 32];

static STATE_BASE: Lazy<PathBuf> = Lazy::new(|| {
    #[cfg(not(test))]
    {
        metalos_paths::core_state::metalos().into()
    }
    #[cfg(test)]
    {
        // prevent unused_crate_dependencies in test mode
        let _ = metalos_paths::core_state::metalos();
        tempfile::tempdir().unwrap().into_path()
    }
});

mod __private {
    pub trait Sealed {}
}
trait ThriftState = fbthrift::Serialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolSerializer<bufsize::SizeCounter>,
    > + fbthrift::Serialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolSerializer<bytes::BytesMut>,
    > + fbthrift::Deserialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolDeserializer<std::io::Cursor<Bytes>>,
    >;

/// Any type that can be serialized to disk and loaded later with then unique id.
pub trait State: Sized + Debug {
    /// Convert this state object to a JSON representation
    fn to_json(&self) -> Result<Vec<u8>>;
    /// Convert a JSON representation into this state type
    fn from_json(bytes: Vec<u8>) -> Result<Self>;

    /// Load the staged version of this staged object, if any.
    fn staged() -> Result<Option<Self>> {
        Self::aliased(Alias::Staged)
    }

    /// Load the current version of this staged object, if any.
    fn current() -> Result<Option<Self>> {
        Self::aliased(Alias::Current)
    }

    /// Load an aliased version of this staged object, if any.
    fn aliased(alias: Alias<Self>) -> Result<Option<Self>> {
        crate::load_alias(alias)
    }

    /// Save this state object to disk.
    fn save(&self) -> Result<Token<Self>> {
        crate::save(self)
    }

    /// Load a state object from disk, if it exists.
    fn load(token: Token<Self>) -> Result<Option<Self>> {
        crate::load(token)
    }
}

impl<T> State for T
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
pub struct Token<S>(Sha256Value, PhantomData<S>)
where
    S: State;

impl<S> Clone for Token<S>
where
    S: State,
{
    fn clone(&self) -> Self {
        Token::new(self.0)
    }
}

impl<S> Copy for Token<S> where S: State {}

impl<S> PartialEq for Token<S>
where
    S: State,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S> Eq for Token<S> where S: State {}

impl<S> std::fmt::Debug for Token<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("type", &std::any::type_name::<S>())
            .field("token", &hex::encode(&self.0))
            .finish()
    }
}

impl<S> std::fmt::Display for Token<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}::{}",
            &std::any::type_name::<S>(),
            hex::encode(self.0)
        )
    }
}

unsafe impl<S> Send for Token<S> where S: State {}
unsafe impl<S> Sync for Token<S> where S: State {}

impl<S> std::str::FromStr for Token<S>
where
    S: State,
{
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let (ty, id_str) = s
            .rsplit_once("::")
            .with_context(|| format!("'{}' missing '::' separator", s))?;
        ensure!(
            ty == std::any::type_name::<S>(),
            "expected type '{}', got '{}'",
            std::any::type_name::<S>(),
            ty
        );
        let id =
            hex::decode(id_str).with_context(|| format!("'{}' is not a hex sha256", id_str))?;
        let id = id
            .try_into()
            .map_err(|_| anyhow!("'{}' is not the correct sha256 length", id_str))?;
        Ok(Self(id, PhantomData))
    }
}

#[derive(Debug)]
/// There are a few special cased tokens that hold meaning in MetalOS.
pub enum Alias<S> {
    /// The most recently staged version of a state variable.
    Staged,
    /// The most recently committed version of a state variable.
    Current,
    /// A custom string instead of the preset values.
    Custom(String, PhantomData<S>),
}

impl<S> Alias<S> {
    fn path(&self) -> PathBuf {
        let filename = format!("{}-{}.json", std::any::type_name::<S>(), self);
        STATE_BASE.join(filename)
    }

    pub fn custom(alias: String) -> Self {
        Self::Custom(alias, PhantomData)
    }
}

impl<S> Clone for Alias<S> {
    fn clone(&self) -> Self {
        match self {
            Self::Staged => Self::Staged,
            Self::Current => Self::Current,
            Self::Custom(s, _) => Self::Custom(s.clone(), PhantomData),
        }
    }
}

impl<S> std::fmt::Display for Alias<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Staged => "staged",
            Self::Current => "current",
            Self::Custom(s, _) => s,
        })
    }
}

impl<S> std::str::FromStr for Alias<S> {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, std::convert::Infallible> {
        Ok(match s {
            "staged" => Self::Staged,
            "current" => Self::Current,
            _ => Self::custom(s.to_string()),
        })
    }
}

impl<S> PartialEq for Alias<S> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Staged, Self::Staged) => true,
            (Self::Current, Self::Current) => true,
            (Self::Custom(s, _), Self::Custom(o, _)) => s == o,
            _ => false,
        }
    }
}

impl<S> Eq for Alias<S> {}

impl<S> Token<S>
where
    S: State,
{
    fn new(hash: Sha256Value) -> Self {
        Self(hash, PhantomData)
    }

    fn path(&self) -> PathBuf {
        // the type name is used to provide somewhat human-readable information
        // about the files on disk (eg if someone runs `ls`)
        let filename = format!(
            "{}-{}.json",
            std::any::type_name::<S>(),
            hex::encode(self.0)
        );
        STATE_BASE.join(filename)
    }

    /// Mark this token as the staged version of a state item.
    ///
    /// See also [commit](Token::commit).
    pub fn stage(&self) -> Result<Alias<S>> {
        self.alias(Alias::Staged)
    }

    /// Mark this token as the current version of a state item.
    ///
    /// Typically precededed by [stage](Token::stage), but this is not required.
    /// [stage](Token::stage) and [commit](Token::commit) hold special meaning
    /// and can be used to retrieve states without knowing the unique [Token].
    pub fn commit(&self) -> Result<Alias<S>> {
        self.alias(Alias::Current)
    }

    /// Assign an Alias to this state item.
    pub fn alias(&self, alias: Alias<S>) -> Result<Alias<S>> {
        crate::alias(*self, alias)
    }

    /// Get a file:// uri that points to this config
    pub fn uri(&self) -> Url {
        Url::from_file_path(self.path())
            .expect("Token::path is always absolute so this cannot fail")
    }
}

/// Persist a new version of a state type, getting back a unique key to later
/// load it with.
fn save<S>(state: &S) -> Result<Token<S>>
where
    S: State,
{
    let state = state
        .to_json()
        .with_context(|| format!("while serializing {:?}", state))?;
    let sha: [u8; 32] = Sha256::digest(&state).into();
    let token = Token::new(sha);
    let p = token.path();
    std::fs::write(&p, &state).with_context(|| format!("while serializing to {}", p.display()))?;
    Ok(token)
}

/// it will be replaced.
fn alias<S>(token: Token<S>, alias: Alias<S>) -> Result<Alias<S>>
where
    S: State,
{
    let alias_path = alias.path();
    std::fs::remove_file(&alias_path)
        .or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
        .with_context(|| format!("while removing existing alias {}", alias_path.display()))?;
    symlink(token.path(), &alias_path).with_context(|| {
        format!(
            "while symlinking alias {} -> {}",
            alias_path.display(),
            token.path().display()
        )
    })?;
    Ok(alias)
}

/// Load a specific version of a state type, using the key returned by [save]
fn load<S>(token: Token<S>) -> Result<Option<S>>
where
    S: State,
{
    match std::fs::read(token.path()) {
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(anyhow::Error::from(e)
                    .context(format!("while opening {}", token.path().display()))),
            }
        }
        Ok(bytes) => S::from_json(bytes)
            .map(Some)
            .with_context(|| format!("while deserializing {}", token.path().display())),
    }
}

/// Load an aliased version of a state type.
fn load_alias<S>(alias: Alias<S>) -> Result<Option<S>>
where
    S: State,
{
    let alias_path = alias.path();
    match std::fs::read(&alias_path) {
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(e).context(format!("while opening {}", alias_path.display())),
        },
        Ok(bytes) => S::from_json(bytes)
            .map(Some)
            .with_context(|| format!("while deserializing {}", alias_path.display())),
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use anyhow::Context;
    use anyhow::Result;
    use example::Example;

    use super::*;

    #[test]
    fn parse() -> Result<()> {
        assert_eq!(
            Token::new(
                hex::decode("f40cd21f276e47d533371afce1778447e858eb5c9c0c0ed61c65f5c5d57caf63")
                    .unwrap()
                    .try_into()
                    .unwrap()
            ),
            "example::types::Example::f40cd21f276e47d533371afce1778447e858eb5c9c0c0ed61c65f5c5d57caf63"
                .parse::<Token<Example>>()
                .unwrap()
        );
        assert_eq!(
            "'not-hex' is not a hex sha256",
            "example::types::Example::not-hex"
                .parse::<Token<Example>>()
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            "'deadbeef' is not the correct sha256 length",
            "example::types::Example::deadbeef"
                .parse::<Token<Example>>()
                .unwrap_err()
                .to_string()
        );
        Ok(())
    }

    #[test]
    fn current() -> Result<()> {
        std::fs::create_dir_all(STATE_BASE.deref())?;
        let current = Example::current().context("while loading non-existent current")?;
        assert_eq!(None, current);
        let token = Example {
            hello: "world".into(),
        }
        .save()
        .context("while saving")?;
        token.commit().context("while writing current alias")?;
        let current = Example::current().context("while loading current")?;
        assert_eq!(
            Some(Example {
                hello: "world".into()
            }),
            current
        );
        Ok(())
    }

    #[test]
    fn custom_alias() -> Result<()> {
        let current = Example::aliased(Alias::custom("myalias".to_string()))
            .context("while loading non-existent alias")?;
        assert_eq!(None, current);
        let token = Example {
            hello: "world".into(),
        }
        .save()
        .context("while saving")?;
        token
            .alias(Alias::custom("myalias".to_string()))
            .context("while writing current alias")?;
        let current = Example::aliased(Alias::custom("myalias".to_string()))
            .context("while loading custom alias")?;
        assert_eq!(
            Some(Example {
                hello: "world".into()
            }),
            current
        );
        Ok(())
    }

    #[test]
    fn save_load_thrift() -> Result<()> {
        std::fs::create_dir_all(STATE_BASE.deref())?;
        let t = Example {
            hello: "world".into(),
        };
        let token = t.save().context("while saving")?;
        let loaded = Example::load(token).context("while loading")?;
        assert_eq!(Some(t), loaded);
        Ok(())
    }
}
