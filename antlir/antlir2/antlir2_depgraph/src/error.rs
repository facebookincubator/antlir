/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fmt::Display;

use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::Validator;
use antlir2_features::Feature;

use crate::Result;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cycle in dependency graph:\n{0}")]
    Cycle(Cycle),
    #[error("{item:?} is provided by multiple features: {features:#?}")]
    Conflict {
        item: Item,
        features: BTreeSet<Feature>,
    },
    #[error("{key:?} is required by {required_by:#?} but was never provided")]
    MissingItem { key: ItemKey, required_by: Feature },
    #[error(
        "{item:?} does not satisfy the validation rules: {validator:?} as required by {required_by:#?}"
    )]
    Unsatisfied {
        item: Item,
        validator: Validator,
        required_by: Feature,
    },
    #[error("failure determining 'provides': {0}")]
    Provides(String),
    #[error("failure determining 'requires': {0}")]
    Requires(String),
    #[error("failed to deserialize feature data: {0}")]
    DeserializeFeature(serde_json::Error),
    #[error("failed to (de)serialize graph data: {0}")]
    GraphSerde(serde_json::Error),
    #[error(transparent)]
    Plugin(#[from] antlir2_features::Error),
    #[error("facts db error: {0}")]
    Facts(#[from] antlir2_facts::Error),
    #[error("sqlite error: {err} context: {context}")]
    Sqlite {
        err: rusqlite::Error,
        context: String,
    },
}

impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::Sqlite {
            err: value,
            context: "<none provided>".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle(pub(crate) Vec<Feature>);

impl Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for feature in &self.0 {
            writeln!(f, "  {feature:?}")?;
        }
        Ok(())
    }
}

pub(crate) trait ContextExt<T>: Sized {
    fn context<S: Display>(self, context: S) -> Result<T>;
    fn with_context<F: FnOnce() -> S, S: Display>(self, with_context: F) -> Result<T> {
        self.context(with_context())
    }
}

impl<T> ContextExt<T> for rusqlite::Result<T> {
    fn context<S: Display>(self, context: S) -> Result<T> {
        self.map_err(|err| Error::Sqlite {
            err,
            context: context.to_string(),
        })
    }

    fn with_context<F: FnOnce() -> S, S: Display>(self, with_context: F) -> Result<T> {
        self.map_err(|err| Error::Sqlite {
            err,
            context: with_context().to_string(),
        })
    }
}
