/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use image_test_lib::KvPair;
use serde_json::json;
use tracing::debug;
use tracing::error;

/// Format the Command for printing
pub(crate) fn format_command(command: &Command) -> String {
    let program = command.get_program().to_string_lossy().to_string();
    let args: Vec<_> = command
        .get_args()
        .map(|x| x.to_string_lossy().to_string())
        .collect();
    format!("Program: `{}`. Args: `{:?}`", program, args)
}

/// Log the command being executed, unless it can't be decoded.
pub(crate) fn log_command(command: &mut Command) -> &mut Command {
    debug!("Executing command: {}", format_command(command));
    command
}

/// The main goal is to redirect any output to tracing so they don't show up by
/// default unless the command fails. It's suitable for blocking commands that
/// finish fast.
pub(crate) fn run_command_capture_output(command: &mut Command) -> Result<(), std::io::Error> {
    let output = log_command(command).output()?;
    let stdout = String::from_utf8(output.stdout).map_err(|e| {
        std::io::Error::new(ErrorKind::Other, format!("Failed to decode stdout: {e}"))
    })?;
    let stderr = String::from_utf8(output.stderr).map_err(|e| {
        std::io::Error::new(ErrorKind::Other, format!("Failed to decode stderr: {e}"))
    })?;
    if !output.status.success() {
        error!(
            "Command {} failed\nStdout: {}\nStderr: {}",
            command.get_program().to_string_lossy(),
            stdout,
            stderr
        );
        return Err(std::io::Error::new(
            ErrorKind::Other,
            format!("Failed to execute command: {:?}", command),
        ));
    }
    if !stdout.is_empty() {
        debug!(stdout);
    }
    if !stderr.is_empty() {
        debug!(stderr);
    }
    Ok(())
}

/// Return a path to record debugging data. When invoked under tpx, this will be
/// uploaded as an artifact.
pub(crate) fn create_tpx_logs(
    name: &str,
    description: &str,
) -> Result<Option<PathBuf>, std::io::Error> {
    // If tpx has provided this artifacts dir, put the logs there so they get
    // uploaded along with the test results
    if let Some(artifacts_dir) = std::env::var_os("TEST_RESULT_ARTIFACTS_DIR") {
        fs::create_dir_all(&artifacts_dir)?;
        let dst = Path::new(&artifacts_dir).join(format!("{}.txt", name));
        // The artifact metadata is set up before running the test so that it
        // still gets uploaded even in case of a timeout
        if let Some(annotations_dir) = std::env::var_os("TEST_RESULT_ARTIFACT_ANNOTATIONS_DIR") {
            fs::create_dir_all(&annotations_dir)?;
            fs::write(
                Path::new(&annotations_dir).join(format!("{}.txt.annotation", name)),
                json!({
                    "type": {
                        "generic_text_log": {},
                    },
                    "description": description,
                })
                .to_string(),
            )?;
        }
        Ok(Some(dst))
    } else {
        Ok(None)
    }
}

/// Convert a list of env names into KvPair with its values
pub(crate) fn env_names_to_kvpairs(env_names: Vec<String>) -> Vec<KvPair> {
    let mut names: HashSet<_> = env_names.into_iter().collect();
    // If these env exist, always pass them through.
    ["RUST_LOG", "RUST_BACKTRACE", "ANTLIR_BUCK"]
        .iter()
        .for_each(|name| {
            names.insert(name.to_string());
        });
    names
        .iter()
        .filter_map(|name| match std::env::var(name) {
            Ok(value) => Some(KvPair::from((name, value))),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
/// Helper function for converting qemu args to a single string for asserting in tests.
/// This is usually only needed for string only functions like `contains`.
pub(crate) fn qemu_args_to_string(args: &[std::ffi::OsString]) -> String {
    args.join(std::ffi::OsStr::new(" "))
        .to_str()
        .expect("Invalid unicode")
        .to_string()
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::env;
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn test_format_command() {
        assert_eq!(
            format_command(&Command::new("hello")),
            "Program: `hello`. Args: `[]`",
        );
        assert_eq!(
            format_command(Command::new("hello").arg("world")),
            format!("Program: `hello`. Args: `{:?}`", vec!["world"]),
        );
    }

    struct EnvTest {
        envs: Vec<(&'static str, &'static str)>,
        passenv: Vec<&'static str>,
        result: HashMap<String, OsString>,
    }

    const ALLOWED_ENV_NAMES: &[&str] = &[
        "RUST_LOG",
        "RUST_BACKTRACE",
        "ANTLIR_BUCK",
        "TEST_PILOT_A",
        "OTHER",
    ];

    impl EnvTest {
        fn new(
            envs: Vec<(&'static str, &'static str)>,
            passenv: Vec<&'static str>,
            result: Vec<(&'static str, &'static str)>,
        ) -> Self {
            // We have to clear all envs across tests, so we must know the ones to clear.
            envs.iter().for_each(|(name, _)| {
                assert!(
                    ALLOWED_ENV_NAMES.contains(name),
                    "{name} not allowed for testing"
                )
            });
            Self {
                envs,
                passenv,
                result: result
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            }
        }

        fn test(&self) {
            ALLOWED_ENV_NAMES.iter().for_each(|name| {
                env::remove_var(name);
            });
            self.envs.iter().for_each(|(name, val)| {
                env::set_var(name, val);
            });
            let result: HashMap<_, _> =
                env_names_to_kvpairs(self.passenv.iter().map(|s| s.to_string()).collect())
                    .drain(..)
                    .map(|pair| (pair.key, pair.value))
                    .collect();
            assert_eq!(result, self.result);
        }
    }

    #[test]
    fn test_env_names_to_kvpairs() {
        [
            EnvTest::new(vec![], vec![], vec![]),
            // Reads nothing
            EnvTest::new(
                vec![],
                vec!["RUST_LOG", "ANTLIR_BUCK", "TEST_PILOT_A", "OTHER"],
                vec![],
            ),
            // Always pass through
            EnvTest::new(
                vec![
                    ("RUST_LOG", "info"),
                    ("ANTLIR_BUCK", "1"),
                    ("TEST_PILOT_A", "A"),
                    ("OTHER", "other"),
                ],
                vec![],
                vec![("RUST_LOG", "info"), ("ANTLIR_BUCK", "1")],
            ),
            // Selection
            EnvTest::new(
                vec![("TEST_PILOT_A", "A"), ("OTHER", "other")],
                vec!["TEST_PILOT_A"],
                vec![("TEST_PILOT_A", "A")],
            ),
            // Mixed
            EnvTest::new(
                vec![
                    ("RUST_LOG", "info"),
                    ("TEST_PILOT_A", "A"),
                    ("OTHER", "other"),
                ],
                vec!["TEST_PILOT_A"],
                vec![("TEST_PILOT_A", "A"), ("RUST_LOG", "info")],
            ),
        ]
        .iter()
        .enumerate()
        .for_each(|(i, test)| {
            println!("Running test #{i}");
            test.test();
        });
    }
}
