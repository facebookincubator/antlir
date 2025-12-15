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
use anyhow::Context;
use anyhow::Result;
use shape::ShapePath;
use target::target_t;
use test_shape::character_collection_t;
use test_shape::character_t;
use test_shape::friend_t;
use test_shape::inner;
use test_shape::target_with_path_t;
use test_shape::thrift_new;
use test_shape::thrift_old;
use test_shape::union_new;
use test_shape::union_old;
use test_shape::with_default_trait;
use test_shape::with_optional_int;

// @oss-disable: const TARGET_PREFIX: &str = "fbcode//antlir/bzl/tests/shapes";
const TARGET_PREFIX: &str = "antlir//antlir/bzl/tests/shapes"; // @oss-enable

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

#[test]
fn thrift() -> Result<()> {
    let characters_str = std::env::var("characters").context("missing 'characters' env var")?;
    let mut characters: character_collection_t = serde_json::from_str(&characters_str)
        .with_context(|| format!("failed to parse json '{}'", characters_str))?;
    let luke = characters.characters.remove(0);
    let after_roundtrip =
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(&luke))?;
    assert_eq!(luke, after_roundtrip);

    assert_eq!(
        serde_json::json!({
            "name":"Luke Skywalker",
            "metadata":{"species":"human"},
            "affiliations": {
                "faction":"Rebellion"
            },
            "appears_in":[4,5,6],
            "friends":[
                {"name":"Han Solo"},
                {"name":"Leia Organa"},
                {"name":"C-3PO"},
            ],
            "personnel_file":"/rebellion/luke_skywalker.txt",
            "weapon":{
                "lightsaber_t":{
                    "color": "green",
                    "target":{
                        "name": format!("{TARGET_PREFIX}:luke-lightsaber"),
                        "path": "",
                    }
                }
            }
        }),
        serde_json::from_slice::<serde_json::Value>(&fbthrift::simplejson_protocol::serialize(
            &luke
        ))
        .expect("failed to round trip through thrift")
    );

    Ok(())
}

#[test]
fn thrift_compat() {
    assert_eq!(
        thrift_new {
            foo: 42,
            baz: Some("baz".into()),
            qux: None
        },
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(thrift_old {
            foo: 42,
            bar: Some("hello-bar".into()),
        }))
        .expect("could not deserialize thrift_old as thrift_new")
    );
    assert_eq!(
        thrift_old { foo: 42, bar: None },
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(thrift_new {
            foo: 42,
            baz: Some("hello-baz".into()),
            qux: Some(true),
        }))
        .expect("could not deserialize thrift_new as thrift_old")
    );
    assert_eq!(
        union_old::Int(42),
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(
            union_new::Int(42)
        ))
        .expect("could not deserialize union_new as union_old"),
    );
    assert_eq!(
        union_new::Int(42),
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(
            union_old::Int(42)
        ))
        .expect("could not deserialize union_old as union_new"),
    );
}

#[test]
fn optional_int() -> Result<()> {
    let s: with_optional_int = serde_json::from_str(r#"{"optint": null}"#)?;
    assert_eq!(s, with_optional_int { optint: None });
    let s: with_optional_int = serde_json::from_str(r#"{"optint": 42}"#)?;
    assert_eq!(s, with_optional_int { optint: Some(42) });
    let s: with_optional_int =
        serde_json::from_str(r#"{"foo": "bar"}"#).context("failed to deser")?;
    assert_eq!(s, with_optional_int { optint: None });
    Ok(())
}

#[test]
fn default_trait() -> Result<()> {
    let with_default: with_default_trait = Default::default();

    assert_eq!(
        with_default,
        with_default_trait {
            a: Some("abc".to_string()),
            b: true,
            c: inner {
                a: Some("def".to_string())
            },
            d: None,
        }
    );

    Ok(())
}

#[test]
fn default_builder() -> Result<()> {
    let with_default = with_default_trait::builder()
        .a(Some("test".to_string()))
        .build();

    assert_eq!(
        with_default,
        with_default_trait {
            a: Some("test".to_string()),
            b: true,
            c: inner {
                a: Some("def".to_string())
            },
            d: None,
        }
    );
    Ok(())
}

#[test]
fn target_forwards_compatible() {
    let new_from_old: target_t = fbthrift::binary_protocol::deserialize(
        fbthrift::binary_protocol::serialize(target_with_path_t {
            name: "foo//bar:baz".into(),
            path: ShapePath::new("lorem/ipsum/dolor".to_owned()),
        }),
    )
    .expect("failed to deserialize new_from_old");
    assert_eq!(
        new_from_old,
        target_t {
            name: "foo//bar:baz".into(),
            path: Some(ShapePath::new("lorem/ipsum/dolor".into()))
        }
    );
}

#[test]
fn target_backwards_compatible() {
    let old_from_new: target_with_path_t =
        fbthrift::binary_protocol::deserialize(fbthrift::binary_protocol::serialize(target_t {
            name: "foo//bar:baz".into(),
            path: Some(ShapePath::new("".to_owned())),
        }))
        .expect("failed to deserialize old_from_new");
    assert_eq!(
        old_from_new,
        target_with_path_t {
            name: "foo//bar:baz".into(),
            path: ShapePath::new("".to_owned()),
        }
    );
}
