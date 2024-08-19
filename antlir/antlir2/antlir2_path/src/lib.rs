/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

pub trait PathExt {
    /// Remove the leading `/` from a path if it exists
    fn strip_abs(&self) -> &Path;

    /// Join `path` onto this path, but ignore any leading `/`s in `path`.
    fn join_abs<P: AsRef<Path>>(&self, path: P) -> PathBuf;
}

impl PathExt for Path {
    fn strip_abs(&self) -> &Path {
        self.strip_prefix("/").unwrap_or(self)
    }

    fn join_abs<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.join(path.as_ref().strip_abs())
    }
}

impl PathExt for PathBuf {
    fn strip_abs(&self) -> &Path {
        self.strip_prefix("/").unwrap_or(self)
    }

    fn join_abs<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.join(path.as_ref().strip_abs())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("/foo/bar", "foo/bar")]
    #[case("foo/bar", "foo/bar")]
    #[case("////foo/bar/baz", "foo/bar/baz")]
    fn test_strip_abs(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(Path::new(input).strip_abs(), Path::new(expected));
    }

    #[rstest]
    #[case("/foo/bar", "baz", "/foo/bar/baz")]
    #[case("/foo/bar", "/baz", "/foo/bar/baz")]
    #[case("/foo/bar", "///baz/qux", "/foo/bar/baz/qux")]
    fn test_join_abspath(#[case] input: &str, #[case] join: &str, #[case] expected: &str) {
        assert_eq!(Path::new(input).join_abs(join), Path::new(expected));
    }
}
