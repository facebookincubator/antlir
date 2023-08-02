/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
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
        test_cmd: Vec<OsString>,
    },
    Gtest {
        #[clap(long, env = "GTEST_OUTPUT")]
        output: Option<String>,
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
    Pyunit {
        #[clap(long)]
        list_tests: Option<PathBuf>,
        #[clap(long)]
        output: Option<PathBuf>,
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
                list_tests, output, ..
            } => {
                let mut paths = HashSet::new();
                if let Some(p) = list_tests {
                    paths.insert(
                        p.parent()
                            .expect("output file always has parent")
                            .to_owned(),
                    );
                }
                if let Some(p) = output {
                    paths.insert(
                        p.parent()
                            .expect("output file always has parent")
                            .to_owned(),
                    );
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
                mut test_cmd,
                output,
            } => {
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
            } => {
                if let Some(list) = list_tests {
                    test_cmd.push("--list-tests".into());
                    test_cmd.push(list.into());
                }
                if let Some(out) = output {
                    test_cmd.push("--output".into());
                    test_cmd.push(out.into());
                }
                for filter in test_filter {
                    test_cmd.push("--test-filter".into());
                    test_cmd.push(filter);
                }
                test_cmd
            }
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
        env::set_var("GTEST_OUTPUT", "/here/here");
        let arg = TestArgs::parse_from(["test", "gtest", "whatever"]);
        assert_eq!(
            arg.test.output_dirs(),
            HashSet::from([PathBuf::from("/here")])
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec!["whatever", "--gtest_output=/here/here"]
        );
    }

    #[test]
    fn test_pyunit() {
        let arg = TestArgs::parse_from([
            "test",
            "pyunit",
            "whatever",
            "--list-tests",
            "/a/here",
            "--output",
            "/here/here",
            "--test-filter",
            "c",
        ]);
        assert_eq!(
            arg.test.output_dirs(),
            ["/a", "/here"]
                .iter()
                .map(PathBuf::from)
                .collect::<HashSet<_>>(),
        );
        assert_eq!(
            arg.test.into_inner_cmd(),
            vec![
                "whatever",
                "--list-tests",
                "/a/here",
                "--output",
                "/here/here",
                "--test-filter",
                "c",
            ]
        );
    }

    #[test]
    fn test_rust() {
        let arg = TestArgs::parse_from(["test", "rust", "whatever"]);
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever"]);
    }

    #[test]
    fn test_custom() {
        let arg = TestArgs::parse_from(["test", "custom", "whatever"]);
        assert_eq!(arg.test.output_dirs(), HashSet::new());
        assert_eq!(arg.test.into_inner_cmd(), vec!["whatever"]);
    }

    #[test]
    fn test_kvpair() {
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
}
