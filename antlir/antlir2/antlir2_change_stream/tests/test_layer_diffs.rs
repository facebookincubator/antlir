/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use antlir2_change_stream::Change;
use antlir2_change_stream::Contents;
use antlir2_change_stream::Iter;
use antlir2_change_stream::Operation;
use pretty_assertions::assert_eq;

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
struct LossyString(String);

impl Contents for LossyString {
    fn from_file(file: std::fs::File) -> std::io::Result<Self> {
        let mut br = BufReader::new(file);
        let mut buf = Vec::new();
        br.read_to_end(&mut buf)?;
        Ok(Self(String::from_utf8_lossy(&buf).into_owned()))
    }

    fn differs(&mut self, other: &mut Self) -> std::io::Result<bool> {
        Ok(self != other)
    }
}

impl From<&str> for LossyString {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TimestampMode {
    Zero,
    Omit,
}

fn changes_between<C>(
    old: impl AsRef<Path>,
    new: impl AsRef<Path>,
    ts_mode: TimestampMode,
) -> Vec<Change<C>>
where
    C: Contents + 'static,
{
    Iter::diff(old, new)
        .expect("failed to create stream")
        .map(|r| r.expect("failed to get change"))
        .filter_map(|change| match change.operation() {
            // zero out timestamps so these tests can be deterministic while
            // still verifying that a SetTimes change is produced
            Operation::SetTimes { .. } => match ts_mode {
                TimestampMode::Omit => None,
                TimestampMode::Zero => Some(Change::new(change.path().to_owned(), zero_times())),
            },
            _ => Some(change),
        })
        .collect()
}

fn zero_times<C>() -> Operation<C> {
    Operation::SetTimes {
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
    }
}

fn set_times<C>(filename: &str) -> Vec<Change<C>> {
    vec![Change::new(PathBuf::from(filename), zero_times())]
}

fn file_changes<C>(
    path: &str,
    operations: impl IntoIterator<Item = Operation<C>>,
) -> impl Iterator<Item = Change<C>> {
    let path = PathBuf::from(path);
    operations
        .into_iter()
        .map(move |op| Change::new(path.clone(), op))
}

#[test]
fn empty_to_empty() {
    let changes = changes_between::<LossyString>("/empty", "/empty", TimestampMode::Zero);
    assert!(
        changes.is_empty(),
        "no changes should have been detected: {changes:#?}"
    );
}

fn empty_to_some_expected_changes() -> Vec<Change<LossyString>> {
    file_changes("foo", [Operation::Mkdir { mode: 0o755 }])
        .chain(file_changes(
            "foo/barbaz",
            [
                Operation::Symlink {
                    target: "/foo/bar/baz".into(),
                },
                Operation::Chown { uid: 0, gid: 0 },
                zero_times(),
            ],
        ))
        .chain(file_changes("foo/bar", [Operation::Mkdir { mode: 0o755 }]))
        .chain(file_changes(
            "foo/bar/baz",
            [
                Operation::Create { mode: 0o444 },
                Operation::Contents {
                    contents: "Baz\n".into(),
                },
                Operation::SetXattr {
                    name: "user.baz".into(),
                    value: "baz".into(),
                },
                Operation::SetXattr {
                    name: "user.foo".into(),
                    value: "foo".into(),
                },
                Operation::Chown { uid: 0, gid: 0 },
                zero_times(),
            ],
        ))
        .chain(file_changes(
            "foo/bar",
            [Operation::Chown { uid: 0, gid: 0 }, zero_times()],
        ))
        .chain(file_changes(
            "foo",
            [Operation::Chown { uid: 0, gid: 0 }, zero_times()],
        ))
        .chain(set_times(""))
        .collect()
}

#[test]
fn empty_to_some() {
    assert_eq!(
        empty_to_some_expected_changes(),
        changes_between::<LossyString>("/empty", "/some", TimestampMode::Zero)
    );
}

#[test]
fn some_as_new() {
    let mut expected = empty_to_some_expected_changes();
    expected.insert(
        0,
        Change::new(PathBuf::from(""), Operation::Mkdir { mode: 0o755 }),
    );
    expected.insert(
        expected.len() - 1,
        Change::new(PathBuf::from(""), Operation::Chown { uid: 0, gid: 0 }),
    );
    assert_eq!(
        expected,
        Iter::<LossyString>::from_empty("/some")
            .expect("failed to create stream")
            .map(|r| r.expect("failed to get change"))
            .map(|change| match change.operation() {
                Operation::SetTimes { .. } => Change::new(change.path().to_owned(), zero_times()),
                _ => change,
            })
            .collect::<Vec<_>>()
    );
}

#[test]
/// Deletions should be detected, and yielded in the correct order for
/// application: bottom-up so that directories are empty by the time Rmdir is
/// produced.
fn some_to_empty() {
    let expected: Vec<Change<LossyString>> = file_changes("foo/barbaz", [Operation::Unlink])
        .chain(file_changes("foo/bar/baz", [Operation::Unlink]))
        .chain(file_changes("foo/bar", [Operation::Rmdir]))
        .chain(file_changes("foo", [Operation::Rmdir]))
        .chain(set_times(""))
        .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some", "/empty", TimestampMode::Zero)
    );
}

#[test]
fn unlink_file() {
    let expected: Vec<Change<LossyString>> = file_changes("foo/bar/baz", [Operation::Unlink])
        .chain(set_times("foo/bar"))
        .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some", "/unlink-file", TimestampMode::Zero)
    );
}

#[test]
fn touch() {
    let expected: Vec<Change<LossyString>> = set_times("foo/barbaz")
        .into_iter()
        .chain(set_times("foo/bar/baz"))
        .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some-mutation-base", "/touch", TimestampMode::Zero)
    );
}

#[test]
fn chown() {
    let expected: Vec<Change<LossyString>> = file_changes(
        "foo/barbaz",
        // mtime does not change when doing a chown
        [Operation::Chown { uid: 42, gid: 43 }],
    )
    .chain(file_changes(
        "foo/bar/baz",
        [Operation::Chown { uid: 42, gid: 43 }],
    ))
    .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some-mutation-base", "/chown", TimestampMode::Zero)
    );
}

#[test]
fn chmod() {
    let expected: Vec<Change<LossyString>> = file_changes(
        "foo/bar/baz",
        [Operation::Chmod {
            // u+sx
            mode: 0o544 | 0o04000,
        }],
    )
    .chain(file_changes("foo/bar", [Operation::Chmod { mode: 0o700 }]))
    .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some-mutation-base", "/chmod", TimestampMode::Zero)
    );
}

#[test]
fn change_file_contents() {
    let expected: Vec<Change<LossyString>> = file_changes(
        "foo/bar/baz",
        [
            Operation::Contents {
                contents: "Baz\nChanged-Contents\n".into(),
            },
            zero_times(),
        ],
    )
    .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>(
            "/some-mutation-base",
            "/change-file-contents",
            TimestampMode::Zero
        )
    );
}

#[test]
fn retarget_symlink() {
    let expected: Vec<Change<LossyString>> = file_changes(
        "foo/barbaz",
        [
            Operation::Unlink,
            Operation::Symlink {
                target: "/qux".into(),
            },
            zero_times(),
        ],
    )
    .chain(set_times("foo"))
    .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>(
            "/some-mutation-base",
            "/retarget-symlink",
            TimestampMode::Zero
        )
    );
}

#[test]
fn change_xattrs() {
    let expected: Vec<Change<LossyString>> = file_changes(
        "foo/bar/baz",
        [Operation::RemoveXattr {
            name: "user.foo".into(),
        }],
    )
    .chain(file_changes(
        "foo/bar/baz",
        [Operation::SetXattr {
            name: "user.baz".into(),
            value: "qux".into(),
        }],
    ))
    .chain(file_changes(
        "foo/bar/baz",
        [Operation::SetXattr {
            name: "user.qux".into(),
            value: "quux".into(),
        }],
    ))
    .chain(file_changes(
        "foo/bar",
        [Operation::SetXattr {
            name: "user.bar".into(),
            value: "bar".into(),
        }],
    ))
    .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>(
            "/some-mutation-base",
            "/change-xattrs",
            TimestampMode::Zero
        )
    );
}

#[test]
fn file_to_dir() {
    let expected: Vec<Change<LossyString>> = file_changes("foo/bar/baz", [Operation::Unlink])
        .chain(file_changes(
            "foo/bar/baz",
            [Operation::Mkdir { mode: 0o755 }],
        ))
        .chain(file_changes(
            "foo/bar/baz/qux",
            [
                Operation::Create { mode: 0o644 },
                Operation::Contents {
                    contents: "qux\n".into(),
                },
                Operation::Chown { uid: 0, gid: 0 },
                zero_times(),
            ],
        ))
        .chain(file_changes(
            "foo/bar/baz",
            [Operation::Chown { uid: 0, gid: 0 }, zero_times()],
        ))
        .chain(set_times("foo/bar"))
        .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some-mutation-base", "/file-to-dir", TimestampMode::Zero)
    );
}

#[test]
fn dir_to_file() {
    let expected: Vec<Change<LossyString>> = file_changes("foo/bar/baz", [Operation::Unlink])
        .chain(file_changes("foo/bar", [Operation::Rmdir]))
        .chain(file_changes(
            "foo/bar",
            [
                Operation::Create { mode: 0o644 },
                Operation::Contents {
                    contents: "bar\n".into(),
                },
                Operation::Chown { uid: 0, gid: 0 },
            ],
        ))
        .collect();
    assert_eq!(
        expected,
        changes_between::<LossyString>("/some-mutation-base", "/dir-to-file", TimestampMode::Omit)
    );
}
