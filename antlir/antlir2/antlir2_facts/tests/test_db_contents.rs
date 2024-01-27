/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::user::Group;
use antlir2_facts::fact::user::User;
use antlir2_facts::Database;
use tracing_test::traced_test;

fn open_db() -> Database {
    Database::open(
        std::env::var_os("TEST_DB").expect("TEST_DB not set"),
        Default::default(),
    )
    .expect("failed to open db")
}

#[test]
#[traced_test]
fn file() {
    let db = open_db();

    let ent = db
        .get::<DirEntry>(DirEntry::key(Path::new("/feature/foo")))
        .expect("failed to get /feature/foo")
        .expect("/feature/foo did not exist");
    assert_eq!(ent.path(), Path::new("/feature/foo"));
    assert_eq!(ent.uid(), 42);
    assert_eq!(ent.gid(), 43);
    assert_eq!(ent.mode(), 0o100444);
    assert!(matches!(ent, DirEntry::RegularFile(_)));
}

#[test]
#[traced_test]
fn dir() {
    let db = open_db();

    let ent = db
        .get::<DirEntry>(DirEntry::key(Path::new("/feature/bar")))
        .expect("failed to get /feature/bar")
        .expect("/feature/bar did not exist");
    assert_eq!(ent.path(), Path::new("/feature/bar"));
    assert_eq!(ent.uid(), 42);
    assert_eq!(ent.gid(), 43);
    assert_eq!(ent.mode(), 0o40755);
    assert!(matches!(ent, DirEntry::Directory(_)));
}

#[test]
#[traced_test]
fn symlink() {
    let db = open_db();

    let ent = db
        .get::<DirEntry>(DirEntry::key(Path::new("/feature/bar/qux")))
        .expect("failed to get /feature/bar/qux")
        .expect("/feature/bar/qux did not exist");
    assert_eq!(ent.path(), Path::new("/feature/bar/qux"));
    assert_eq!(ent.uid(), 0);
    assert_eq!(ent.gid(), 0);
    assert_eq!(ent.mode(), 0o120777);
    assert!(matches!(ent, DirEntry::Symlink(_)));
    match ent {
        DirEntry::Symlink(symlink) => {
            assert_eq!(symlink.target(), Path::new("/feature/bar/baz"));
            assert_eq!(symlink.raw_target(), Path::new("/feature/bar/baz"));
        }
        _ => unreachable!(),
    }
}

#[test]
#[traced_test]
fn relative_symlink() {
    let db = open_db();

    let ent = db
        .get::<DirEntry>(DirEntry::key(Path::new("/relative-symlink")))
        .expect("failed to get /relative-symlink")
        .expect("/relative-symlink did not exist");
    assert_eq!(ent.path(), Path::new("/relative-symlink"));
    assert_eq!(ent.uid(), 0);
    assert_eq!(ent.gid(), 0);
    assert_eq!(ent.mode(), 0o120777);
    assert!(matches!(ent, DirEntry::Symlink(_)));
    match ent {
        DirEntry::Symlink(symlink) => {
            assert_eq!(symlink.target(), Path::new("/target"));
            assert_eq!(symlink.raw_target(), Path::new("target"));
        }
        _ => unreachable!(),
    }
}

#[test]
#[traced_test]
fn user() {
    let db = open_db();

    let ent = db
        .get::<User>(User::key("antlir"))
        .expect("failed to get user antlir")
        .expect("user antlir did not exist");
    assert_eq!(ent.name(), "antlir");
    assert_eq!(ent.id(), 42);
}

#[test]
#[traced_test]
fn group() {
    let db = open_db();

    let ent = db
        .get::<Group>(Group::key("antlir"))
        .expect("failed to get group antlir")
        .expect("group antlir did not exist");
    assert_eq!(ent.name(), "antlir");
    assert_eq!(ent.id(), 43);
    assert_eq!(ent.members().collect::<Vec<_>>(), &["antlir"]);
}
