/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use thrift_wrapper::{Error, ThriftWrapper};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
struct NewTypedHello(String);

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::MyStruct)]
struct MyStruct {
    url: url::Url,
    hello: String,
    newtyped_hello: NewTypedHello,
    number: i32,
    nested: Nested,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::Nested)]
struct Nested {
    uuid: uuid::Uuid,
}

#[test]
fn to_thrift() {
    let uuid = Uuid::new_v4();
    assert_eq!(
        test_if::MyStruct {
            url: "https://hello/world".into(),
            hello: "world".into(),
            newtyped_hello: "world".into(),
            number: 42,
            nested: test_if::Nested {
                uuid: uuid.to_simple().to_string(),
            }
        },
        MyStruct {
            url: "https://hello/world".parse().unwrap(),
            hello: "world".into(),
            newtyped_hello: NewTypedHello("world".into()),
            number: 42,
            nested: Nested { uuid }
        }
        .into(),
    )
}

#[test]
fn from_thrift() -> Result<()> {
    let uuid = Uuid::new_v4();
    assert_eq!(
        MyStruct {
            url: "https://hello/world".parse().unwrap(),
            hello: "world".into(),
            newtyped_hello: NewTypedHello("world".into()),
            number: 42,
            nested: Nested { uuid }
        },
        MyStruct::try_from(test_if::MyStruct {
            url: "https://hello/world".into(),
            hello: "world".into(),
            newtyped_hello: "world".into(),
            number: 42,
            nested: test_if::Nested {
                uuid: uuid.to_simple().to_string()
            }
        })?,
    );
    Ok(())
}

#[test]
fn from_thrift_bad_nested_field() -> Result<()> {
    match MyStruct::try_from(test_if::MyStruct {
        url: "https://hello/world".into(),
        hello: "world".into(),
        newtyped_hello: "world".into(),
        number: 42,
        nested: test_if::Nested {
            uuid: "notauuid".into(),
        },
    }) {
        Err(Error::Nested { field, error }) => {
            assert_eq!(field, "nested.uuid");
            assert_eq!(error.to_string(), "'notauuid' is not a valid uuid");
        }
        _ => panic!("should have failed"),
    };
    Ok(())
}

#[test]
fn from_thrift_bad_top_field() -> Result<()> {
    let uuid = Uuid::new_v4();
    match MyStruct::try_from(test_if::MyStruct {
        url: "notaurl".into(),
        hello: "world".into(),
        newtyped_hello: "world".into(),
        number: 42,
        nested: test_if::Nested {
            uuid: uuid.to_string(),
        },
    }) {
        Err(Error::Nested { field, error }) => {
            assert_eq!(field, "url");
            assert_eq!(error.to_string(), "'notaurl' is not a valid url");
        }
        _ => panic!("should have failed"),
    };
    Ok(())
}
