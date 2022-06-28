/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! These tests don't have to be anywhere near as exhaustive as those of
//! test_shape.py, because rust gives so much type-safety already. Basically, as
//! long as `serde_json::from_str` is able to load the `character_collection_t`,
//! everything else that can be done with shapes are safe.
use anyhow::{Context, Result};
use test_shape::{character_collection_t, character_t, friend_t};

#[test]
fn load() -> Result<()> {
    let characters_str = std::env::var("characters").context("missing 'characters' env var")?;
    let mut characters: character_collection_t = serde_json::from_str(&characters_str)
        .with_context(|| format!("failed to parse json '{}'", characters_str))?;
    let luke = characters.characters.remove(0);
    assert_eq!(luke.name, "Luke Skywalker");
    assert_eq!(luke.appears_in, [4, 5, 6]);
    assert_eq!(
        luke.friends,
        ["Han Solo", "Leia Organa", "C-3PO"]
            .into_iter()
            .map(|n| friend_t { name: n.into() })
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn defaults() -> Result<()> {
    // this really checks that the default values are deserialized from a shape
    // input (well-structured from the buck graph)
    let characters_str = std::env::var("characters").context("missing 'characters' env var")?;
    let mut characters: character_collection_t = serde_json::from_str(&characters_str)
        .with_context(|| format!("failed to parse json '{}'", characters_str))?;
    let c3po = characters.characters.remove(2);
    assert_eq!(c3po.name, "C-3PO");
    assert_eq!(c3po.affiliations.faction, "Rebellion");

    // check that the default values are populated even with empty json
    let c3po: character_t =
        serde_json::from_str(r#"{"name": "C-3PO", "appears_in": [1,2,3,4,5,6], "friends": []}"#)
            .context("failed to deserialize json without explicit default values")?;
    assert_eq!(c3po.name, "C-3PO");
    assert_eq!(c3po.affiliations.faction, "Rebellion");

    Ok(())
}
