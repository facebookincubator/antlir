/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use antlir2_facts::Fact;
use antlir2_facts::Key;
use antlir2_facts::RoDatabase;
use antlir2_facts::fact_impl;
use serde::Deserialize;
use serde::Serialize;
use tracing_test::traced_test;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
struct ExtendFacts {
    msg: String,
}

#[fact_impl("test_appears_in_facts::ExtendFacts")]
impl Fact for ExtendFacts {
    fn key(&self) -> Key {
        self.msg.as_bytes().into()
    }
}

fn open_db(name: &str) -> RoDatabase {
    RoDatabase::open(
        buck_resources::get(format!(
            "antlir/antlir2/features/test_only_features/extend_facts/tests/{name}"
        ))
        .unwrap_or_else(|_| panic!("db {name} resource not set")),
    )
    .unwrap_or_else(|_| panic!("failed top open {name}"))
}

fn extend_facts_from_db(db: &RoDatabase) -> HashSet<String> {
    db.iter::<ExtendFacts>()
        .expect("failed to iterate facts")
        .map(|f| f.msg)
        .collect()
}

#[test]
#[traced_test]
fn parent() {
    let db = open_db("parent.db");

    assert_eq!(
        extend_facts_from_db(&db),
        HashSet::from([
            "fact logged from parent".to_string(),
            "planner: fact logged from parent".to_string(),
        ])
    );
}

#[test]
#[traced_test]
fn child() {
    let db = open_db("child.db");

    assert_eq!(
        extend_facts_from_db(&db),
        HashSet::from([
            // the fact from the parent should still exist
            "fact logged from parent".to_string(),
            "planner: fact logged from parent".to_string(),
            // the child fact should have been added
            "fact logged from child".to_string(),
            "planner: fact logged from child".to_string(),
        ])
    );
}
