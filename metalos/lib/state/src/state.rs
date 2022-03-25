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

use std::fs::File;
use std::marker::PhantomData;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

/// Special UUID for the "staged" value which holds special meaning
/// c4dbff2e-2c0a-4e35-85a2-2ccb7dd8d8a9
static UUID_STAGED: Uuid = Uuid::from_u128(261670975858436844194139735623489607849);
/// Special UUID for the "current" value which holds special meaning
/// 80e27f0c-75dd-4855-9ada-addea2d90a1b
static UUID_CURRENT: Uuid = Uuid::from_u128(171317219403732977053801729476395600411);

pub trait State = DeserializeOwned + Serialize;

#[derive(PartialEq, Eq)]
/// Unique reference to a piece of state of a specific type. Can be used to
/// retrieve the state from disk via [load]
pub struct Token<S>(Uuid, PhantomData<S>)
where
    S: State;

impl<S> Clone for Token<S>
where
    S: State,
{
    fn clone(&self) -> Token<S> {
        Token::new(self.0)
    }
}

impl<S> Copy for Token<S> where S: State {}

impl<S> std::fmt::Debug for Token<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("type", &std::any::type_name::<S>())
            .field("token", &self.0)
            .finish()
    }
}

impl<S> std::fmt::Display for Token<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", &std::any::type_name::<S>(), self.0.to_simple())
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
    fn token<S>(self) -> Token<S>
    where
        S: State,
    {
        Token::new(match self {
            Self::Current => UUID_CURRENT,
            Self::Staged => UUID_STAGED,
        })
    }
}

impl<S> Token<S>
where
    S: State,
{
    fn new(uuid: Uuid) -> Self {
        Self(uuid, PhantomData)
    }

    fn path(&self) -> PathBuf {
        // the type name is used to provide somewhat human-readable information
        // about the files on disk (eg if someone runs `ls`)
        let filename = format!("{}-{}.json", std::any::type_name::<S>(), &self.0);
        metalos_paths::metalos_state().join(filename)
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
pub fn save<S>(state: S) -> Result<Token<S>>
where
    S: State,
{
    let token = Token::new(Uuid::new_v4());
    let p = token.path();
    let mut f = File::create(&p).with_context(|| format!("while creating {}", p.display()))?;
    serde_json::to_writer(&mut f, &state)
        .with_context(|| format!("while serializing {}", p.display()))?;
    Ok(token)
}

/// Save this specific token as a special [Alias]. If this alias already exists,
/// it will be replaced.
fn alias<S>(token: Token<S>, alias: Alias) -> Result<()>
where
    S: State,
{
    let alias: Token<S> = alias.token();
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
pub fn load<S>(token: Token<S>) -> Result<Option<S>>
where
    S: State,
{
    let mut f =
        match File::open(token.path()) {
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => return Ok(None),
                _ => Err(anyhow::Error::from(e)
                    .context(format!("while opening {}", token.path().display()))),
            },
            Ok(f) => Ok(f),
        }?;
    serde_json::from_reader(&mut f)
        .with_context(|| format!("while deserializing {}", token.path().display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use metalos_macros::containertest;
    use serde::Deserialize;

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

    #[containertest]
    fn current() -> Result<()> {
        std::fs::create_dir_all(metalos_paths::metalos_state())?;
        let current: Option<ExampleState> =
            load(Token::current()).context("while loading non-existent current")?;
        assert_eq!(None, current);
        let token = save(ExampleState {
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

    #[containertest]
    fn kv() -> Result<()> {
        std::fs::create_dir_all(metalos_paths::metalos_state())?;
        let token = save(ExampleState {
            hello: "world".into(),
        })
        .context("while saving")?;
        let loaded = load(token).context("while loading current")?;
        assert_eq!(
            Some(ExampleState {
                hello: "world".into()
            }),
            loaded
        );
        Ok(())
    }
}
