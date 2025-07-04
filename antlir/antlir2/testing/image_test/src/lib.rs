/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;
use thiserror::Error;

#[derive(Parser, Clone, Debug)]
/// Unittest macros can pass in different flags for the test commands for
/// different type of tests. However, we sometimes need to extract information
/// from the command. This enum parses the expected flags for each type.
pub enum Test {
    Custom {
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
    Gtest {
        test: PathBuf,
        #[clap(long = "gtest_output", env = "GTEST_OUTPUT", require_equals = true)]
        output: Option<String>,
        #[clap(long = "gtest_list_tests")]
        gtest_list_tests: bool,
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
    Pyunit {
        #[clap(long, default_value=None,default_missing_value=Some(""),num_args=0..=1)]
        list_tests: Option<String>,
        #[clap(long)]
        output: Option<PathBuf>,
        #[clap(long)]
        output_dirs: Vec<PathBuf>,
        #[clap(long)]
        test_filter: Vec<OsString>,
        test_cmd: Vec<OsString>,
    },
    Rust {
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
}

impl Test {
    /// Some tests need to write to output paths on the host. Instead of a
    /// complicated fd-passing dance in the name of isolation purity, we just
    /// mount the parent directories of the output files so that the inner test
    /// can do writes just as tpx expects.
    pub fn output_dirs(&self) -> HashSet<PathBuf> {
        match self {
            Self::Custom { .. } => HashSet::new(),
            Self::Gtest { output, .. } => match output {
                Some(output) => {
                    let path = Path::new(match output.split_once(':') {
                        Some((_format, path)) => path,
                        None => output.as_str(),
                    });
                    HashSet::from([path
                        .parent()
                        .expect("output file always has parent")
                        .to_owned()])
                }
                None => HashSet::new(),
            },
            Self::Rust { .. } => HashSet::new(),
            Self::Pyunit {
                list_tests,
                output,
                output_dirs,
                ..
            } => {
                let mut paths = HashSet::new();
                if let Some(p) = list_tests {
                    if !p.is_empty() {
                        paths.insert(
                            PathBuf::from(p)
                                .parent()
                                .expect("output file always has parent")
                                .to_owned(),
                        );
                    }
                }
                if let Some(p) = output {
                    paths.insert(
                        p.parent()
                            .expect("output file always has parent")
                            .to_owned(),
                    );
                }
                // Add all output_dirs directly
                for dir in output_dirs {
                    paths.insert(dir.clone());
                }
                paths
            }
        }
    }

    /// Re-construct the unittest command
    pub fn into_inner_cmd(self) -> Vec<OsString> {
        match self {
            Self::Custom { test_cmd } => test_cmd,
            Self::Gtest {
                test,
                mut test_cmd,
                gtest_list_tests,
                output,
            } => {
                test_cmd.insert(0, test.into());
                if gtest_list_tests {
                    test_cmd.push("--gtest_list_tests".into());
                }
                if let Some(out) = output {
                    test_cmd.push(format!("--gtest_output={out}").into());
                }
                test_cmd
            }
            Self::Rust { test_cmd } => test_cmd,
            Self::Pyunit {
                mut test_cmd,
                list_tests,
                test_filter,
                output,
                output_dirs,
            } => {
                if let Some(list) = list_tests {
                    test_cmd.push("--list-tests".into());
                    if !list.is_empty() {
                        test_cmd.push(list.into());
                    }
                }
                if let Some(out) = output {
                    test_cmd.push("--output".into());
                    test_cmd.push(out.into());
                }
                for dir in output_dirs {
                    test_cmd.push("--output-dirs".into());
                    test_cmd.push(dir.into());
                }
                for filter in test_filter {
                    test_cmd.push("--test-filter".into());
                    test_cmd.push(filter);
                }
                test_cmd
            }
        }
    }

    /// Check if the command is for listing tests
    pub fn is_list_tests(&self) -> bool {
        match self {
            Self::Custom { .. } => false,
            Self::Gtest {
                gtest_list_tests, ..
            } => *gtest_list_tests,
            Self::Rust { test_cmd, .. } => test_cmd.contains(&OsString::from("--list".to_string())),
            Self::Pyunit { list_tests, .. } => list_tests.is_some(),
        }
    }
}

#[derive(Error, Debug)]
pub enum ParsingError {
    #[error("Failed to parse KvPair: {0}")]
    KvPairError(String),
}

/// Parse "Key=Value" pair use for env parameter
#[derive(Debug, Clone, PartialEq)]
pub struct KvPair {
    pub key: String,
    pub value: OsString,
}

impl FromStr for KvPair {
    type Err = ParsingError;

    fn from_str(s: &str) -> Result<Self, ParsingError> {
        match s.split_once('=') {
            Some((key, value)) => Ok(Self {
                key: key.to_owned(),
                value: value.trim_matches('"').into(),
            }),
            None => Err(ParsingError::KvPairError(format!(
                "expected = separated kv pair, got '{s}'"
            ))),
        }
    }
}

impl<K, V> From<(K, V)> for KvPair
where
    K: AsRef<str> + Clone + std::fmt::Display,
    V: AsRef<OsStr> + Clone,
    OsString: From<V>,
{
    fn from(kv: (K, V)) -> Self {
        KvPair {
            key: kv.0.to_string(),
            value: OsString::from(kv.1),
        }
    }
}

impl KvPair {
    pub fn to_os_string_for_env(&self) -> OsString {
        let mut value = OsString::new();
        value.push(OsStr::new("'"));
        value.push(&self.key);
        value.push(OsStr::new("'"));
        value.push(OsStr::new("="));
        value.push(OsStr::new("'"));
        value.push(&self.value);
        value.push(OsStr::new("'"));
        value
    }
}

#[cfg(test)]
mod test {
    use std::env;

    use clap::Parser;

    use super::*;

    #[derive(Parser, Debug)]
    struct TestArgs {
        #[clap(subcommand)]
        test: Test,
    }

    #[test]
    fn test_gtest() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("GTEST_OUTPUT", "/here/here") };
        let arg = TestArgs::parse_from(["test", "gtest", "/path/to/the/test"]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/here")])
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec!["/path/to/the/test", "--gtest_output=/here/here"]
        );

        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::remove_var("GTEST_OUTPUT") };
        let arg =
            TestArgs::parse_from(["test", "gtest", "/path/to/the/test", "--gtest_list_tests"]);
        assert!(arg.test.is_list_tests());
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec!["/path/to/the/test", "--gtest_list_tests"],
        );

        let arg = TestArgs::parse_from([
            "test",
            "gtest",
            "/path/to/the/test",
            "--gtest_list_tests",
            "--gtest_output=json:/foo/bar",
        ]);
        assert!(arg.test.is_list_tests());
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/foo")]),
            "{arg:#?}",
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec![
                "/path/to/the/test",
                "--gtest_list_tests",
                "--gtest_output=json:/foo/bar"
            ]
        );

        let arg = TestArgs::parse_from([
            "test",
            "gtest",
            "/path/to/the/test",
            "--gtest_catch_exceptions=0",
            "--gtest_filter=foo/bar/baz",
        ]);
        assert!(!arg.test.is_list_tests());
        assert!(arg.test.output_dirs().is_empty());
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec![
                "/path/to/the/test",
                "--gtest_catch_exceptions=0",
                "--gtest_filter=foo/bar/baz",
            ]
        );
    }

    #[test]
    fn test_pyunit() {
        let arg = TestArgs::parse_from([
            "test",
            "pyunit",
            "whatever",
            "--output",
            "/here/here",
            "--test-filter",
            "c",
        ]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/here")])
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec!["whatever", "--output", "/here/here", "--test-filter", "c",]
        );

        let arg = TestArgs::parse_from(["test", "pyunit", "whatever", "--list-tests", "/a/here"]);
        assert!(arg.test.is_list_tests());
        assert_eq!(arg.test.output_dirs(), HashSet::from([PathBuf::from("/a")]));
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec!["whatever", "--list-tests", "/a/here"]
        );

        let arg = TestArgs::parse_from(["test", "pyunit", "whatever", "--list-tests"]);
        assert!(arg.test.is_list_tests());
        assert!(arg.test.output_dirs().is_empty());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever", "--list-tests"]);

        // Test with output_dirs
        let arg = TestArgs::parse_from([
            "test",
            "pyunit",
            "whatever",
            "--output-dirs",
            "/tmp/foo",
            "--output-dirs",
            "/tmp/other",
        ]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/tmp/foo"), PathBuf::from("/tmp/other")])
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec![
                "whatever",
                "--output-dirs",
                "/tmp/foo",
                "--output-dirs",
                "/tmp/other",
            ]
        );

        // Test with both output and output_dirs
        let arg = TestArgs::parse_from([
            "test",
            "pyunit",
            "whatever",
            "--output",
            "/here/here",
            "--output-dirs",
            "/tmp/foo",
        ]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/here"), PathBuf::from("/tmp/foo")])
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec![
                "whatever",
                "--output",
                "/here/here",
                "--output-dirs",
                "/tmp/foo",
            ]
        );
    }

    #[test]
    fn test_rust() {
        let arg = TestArgs::parse_from(["test", "rust", "whatever"]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever"]);

        let arg = TestArgs::parse_from(["test", "rust", "whatever", "--list"]);
        assert!(arg.test.is_list_tests());
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever", "--list"]);
    }

    #[test]
    fn test_custom() {
        let arg = TestArgs::parse_from(["test", "custom", "whatever", "--list"]);
        assert!(!arg.test.is_list_tests());
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever", "--list"]);
    }

    #[test]
    fn test_kvpair_from_str() {
        #[derive(Parser, Debug)]
        struct KvPairArgs {
            #[clap(long)]
            pair: Vec<KvPair>,
        }
        let arg = KvPairArgs::parse_from(["test", "--pair", "a=b", "--pair", "c=d"]);
        assert_eq!(
            arg.pair,
            vec![
                KvPair {
                    key: "a".into(),
                    value: "b".into(),
                },
                KvPair {
                    key: "c".into(),
                    value: "d".into(),
                }
            ]
        );
    }

    #[test]
    fn test_kvpair_to_os_string() {
        assert_eq!(
            KvPair::from(("a", "b")).to_os_string_for_env(),
            OsString::from("'a'='b'"),
        )
    }
}
