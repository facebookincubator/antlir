/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::rpm::Rpm;
use antlir2_facts::fact::user::Group;
use antlir2_facts::fact::user::User;
use antlir2_facts::RoDatabase;
use tracing_test::traced_test;

fn open_db() -> RoDatabase {
    RoDatabase::open(
        buck_resources::get("antlir/antlir2/antlir2_facts/tests/test_db")
            .expect("test_db resource not set"),
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
    assert_eq!(ent.uid(), 1000);
    assert_eq!(ent.gid(), 1000);
    assert_eq!(ent.mode(), 0o100444);
    assert!(matches!(ent, DirEntry::RegularFile(_)));
}

#[test]
#[traced_test]
fn device_nodes() {
    let db = open_db();

    let ent = db
        .get::<DirEntry>(DirEntry::key(Path::new("/dev/null")))
        .expect("failed to get /dev/null")
        .expect("/dev/null did not exist");
    assert_eq!(ent.path(), Path::new("/dev/null"));
    assert_eq!(ent.uid(), 0);
    assert_eq!(ent.gid(), 0);
    assert_eq!(ent.mode(), 0o100644);
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
    assert_eq!(ent.uid(), 1000);
    assert_eq!(ent.gid(), 1000);
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
    assert_eq!(ent.id(), 1000);
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
    assert_eq!(ent.id(), 1000);
    assert_eq!(ent.members().collect::<Vec<_>>(), &["antlir"]);
}

#[test]
#[traced_test]
fn rpms() {
    let db = open_db();

    let rpms = db
        .iter::<Rpm>()
        .expect("failed to iterate over rpms")
        .collect::<Vec<_>>();
    assert!(rpms.len() > 1, "multiple rpm facts should be found");
    let rpms = rpms
        .into_iter()
        .map(|rpm| (rpm.name().to_owned(), rpm))
        .collect::<HashMap<_, _>>();
    assert!(
        rpms.contains_key("foobar"),
        "explicitly installed rpm should be recorded"
    );
    assert!(
        rpms.contains_key("foo"),
        "rpm installed as a dep should be recorded"
    );
    assert_eq!(rpms.get("foo").map(Rpm::epoch), Some(0), "foo has no epoch");
    assert_eq!(
        rpms.get("foo-epoch").map(Rpm::epoch),
        Some(3),
        "epoch should be recorded"
    );
    assert_eq!(
        rpms.get("antlir2-changelog")
            .expect("antlir2-changelog rpm missing")
            .changelog(),
        Some("- Example changelog\n- CVE-2024-0101"),
    );
}

#[test]
#[traced_test]
fn systemd_unit_file() {
    let db = open_db();

    let ent = db
        .get::<antlir2_systemd::UnitFile>("foo.service")
        .expect("failed to get foo.service")
        .expect("foo.service did not exist");
    assert_eq!(ent.name(), "foo.service");
    assert_eq!(ent.state(), antlir2_systemd::UnitFileState::Static);
}

#[test]
#[traced_test]
fn child_removes_things() {
    let db = RoDatabase::open(
        buck_resources::get("antlir/antlir2/antlir2_facts/tests/child_db")
            .expect("child_db resource not set"),
    )
    .expect("failed to open db");

    assert!(
        db.get::<DirEntry>(DirEntry::key(Path::new("/feature/foo")))
            .expect("failed to get /feature/foo")
            .is_none(),
        "/feature/foo should have been removed"
    );

    assert!(
        db.get::<User>(User::key("antlir"))
            .expect("failed to get user antlir")
            .is_none(),
        "user 'antlir' should have been removed"
    );

    assert!(
        db.get::<Group>(Group::key("antlir"))
            .expect("failed to get group antlir")
            .is_none(),
        "group 'antlir' should have been removed"
    );

    let rpm_names = db
        .iter::<Rpm>()
        .expect("failed to iterate over rpms")
        .map(|r| r.name().to_owned())
        .collect::<HashSet<_>>();
    assert!(
        !rpm_names.contains("foobar"),
        "'foobar' should have been removed"
    );

    assert!(
        db.get::<antlir2_systemd::UnitFile>("foo.service")
            .expect("failed to get foo.service")
            .is_none(),
        "foo.service should have been removed"
    )
}
