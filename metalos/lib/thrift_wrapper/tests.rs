/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use async_trait::async_trait;
use static_assertions as sa;
use thrift_wrapper::thrift_server;
use thrift_wrapper::Error;
use thrift_wrapper::ThriftWrapper;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
struct NewTypedHello(String);

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::MyStruct)]
pub struct MyStruct {
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

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::UnionA)]
struct UnionA {
    foo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::UnionB)]
struct UnionB {
    bar: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::MyUnion)]
enum MyUnion {
    A(UnionA),
    #[thrift_field_name("nEw")]
    B(UnionB),
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

#[test]
fn thrift_union() -> Result<()> {
    assert_eq!(
        MyUnion::A(UnionA { foo: "bar".into() }),
        test_if::MyUnion::a(test_if::UnionA { foo: "bar".into() }).try_into()?
    );
    assert_eq!(
        MyUnion::B(UnionB { bar: 2 }),
        test_if::MyUnion::nEw(test_if::UnionB { bar: 2 }).try_into()?
    );
    match MyUnion::try_from(test_if::MyUnion::UnknownField(42)) {
        Ok(_) => panic!("should have failed"),
        Err(e) => match e {
            Error::Union(42) => (),
            _ => panic!("expected Error::Union(42)"),
        },
    };
    assert_eq!(
        test_if::MyUnion::a(test_if::UnionA { foo: "bar".into() }),
        MyUnion::A(UnionA { foo: "bar".into() }).into(),
    );
    assert_eq!(
        test_if::MyUnion::nEw(test_if::UnionB { bar: 2 }),
        MyUnion::B(UnionB { bar: 2 }).into(),
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(test_if::Exn)]
pub struct Exn {
    pub msg: String,
}

#[thrift_server(thrift = "test_if::server::Svc")]
pub trait Svc: Send + Sync + 'static {
    #[thrift(args = "test_if::MyStruct", ret = "test_if::MyStruct")]
    async fn some_method(&self, arg: MyStruct) -> Result<MyStruct, Exn>;
}

#[derive(Clone)]
struct MyImpl {}

#[async_trait]
impl Svc for MyImpl {
    async fn some_method(&self, arg: MyStruct) -> Result<MyStruct, Exn> {
        if arg.number == 42 {
            Ok(arg)
        } else {
            Err(Exn {
                msg: "only 42 is allowed".into(),
            })
        }
    }
}

sa::assert_impl_all!(SvcServer<MyImpl>: test_if::server::Svc);

#[tokio::test]
async fn thrift_service() -> Result<()> {
    let svc = MyImpl {};

    let input = MyStruct {
        url: "https://hello/world".parse().expect("this is a valid url"),
        hello: "world".into(),
        newtyped_hello: NewTypedHello("world".into()),
        number: 42,
        nested: Nested {
            uuid: Uuid::new_v4(),
        },
    };

    assert_eq!(
        test_if::server::Svc::some_method(&SvcServer(svc.clone()), input.clone().into())
            .await
            .expect("server response should pass"),
        input.clone().into_thrift(),
    );

    // func throws a nice error
    let mut bad = input.clone();
    bad.number = 43;
    match test_if::server::Svc::some_method(&SvcServer(svc.clone()), bad.into())
        .await
        .unwrap_err()
    {
        test_if::services::svc::SomeMethodExn::e(e) => assert_eq!(
            e,
            test_if::Exn {
                msg: "only 42 is allowed".into()
            }
        ),
        other => panic!("expected Exn, got {:?}", other),
    };

    // input cannot be converted to safe struct
    let mut input_cannot_convert = input.clone().into_thrift();
    input_cannot_convert.url = "notaurl".into();
    match test_if::server::Svc::some_method(&SvcServer(svc.clone()), input_cannot_convert)
        .await
        .unwrap_err()
    {
        test_if::services::svc::SomeMethodExn::ApplicationException(aex) => assert_eq!(
            aex.message,
            "argument 'arg' could not be converted to rust repr: error in field url: 'notaurl' is not a valid url"
        ),
        other => panic!("expected ApplicationException, got {:?}", other),
    };

    Ok(())
}
