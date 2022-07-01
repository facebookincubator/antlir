/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use super::pyunit;
use super::rust;

/// Unit test.
#[derive(Debug)]
pub struct Test {
    /// The command used to execute the test.
    pub command: Command,

    /// User-facing identifier for this specific test.
    pub name: String,

    /// Labels/tags associated to this test.
    pub labels: HashSet<String>,

    /// Contacts for further information.
    pub contacts: HashSet<String>,

    /// Which type of test this is.
    pub kind: TestKind,
}

/// Supported types of tests.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    /// Buck's custom `unittest` runner.
    Pyunit,

    /// Buck's custom Rust tester.
    Rust,

    /// Simply runs a command and checks for non-zero exit code.
    /// When running a `(buck_)sh_test`, this tag is automatically inserted.
    #[serde(rename = "custom")]
    Shell,
}

/// Labels which mark buck test targets for automatic (and silent) exclusion.
const EXCLUDED_LABELS: &[&str] = &["disabled", "exclude_test_if_transitive_dep"];

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub attempts: u32,
    pub passed: bool,
    pub duration: Duration,
    pub stdout: String,
    pub stderr: String,
    pub contacts: HashSet<String>,
}

impl Test {
    pub fn run(mut self, retries: u32) -> TestResult {
        let mut attempts = 0;
        loop {
            let time = Instant::now();
            let mut child = self.command.spawn().unwrap();
            child.wait().unwrap();
            let duration = time.elapsed();
            attempts += 1;

            let pass = match self.kind {
                TestKind::Pyunit => pyunit::evaluate(&mut child),
                TestKind::Rust => rust::evaluate(&mut child),
                TestKind::Shell => shell::evaluate(&mut child),
            };

            if pass {
                let mut out = String::new();
                child.stdout.unwrap().read_to_string(&mut out).unwrap();
                let mut err = String::new();
                child.stderr.unwrap().read_to_string(&mut err).unwrap();
                return TestResult {
                    name: self.name,
                    attempts,
                    passed: true,
                    duration,
                    stdout: out,
                    stderr: err,
                    contacts: self.contacts,
                };
            } else {
                if attempts >= 1 + retries {
                    let mut out = String::new();
                    child.stdout.unwrap().read_to_string(&mut out).unwrap();
                    let mut err = String::new();
                    child.stderr.unwrap().read_to_string(&mut err).unwrap();
                    return TestResult {
                        name: self.name,
                        attempts,
                        passed: false,
                        duration,
                        stdout: out,
                        stderr: err,
                        contacts: self.contacts,
                    };
                }
            }
        }
    }
}

// a.k.a. the defaults
pub mod shell {
    use super::make_command;
    use super::Test;
    use super::TestKind;
    use super::TestSpec;
    use std::process::Child;

    /// Builds a singleton test from a given spec.
    pub fn list_tests(spec: TestSpec) -> Vec<Test> {
        let command = make_command(
            spec.command[0].clone(),
            spec.command[1..].to_vec(),
            spec.env,
            spec.cwd,
        );
        let test = Test {
            command,
            name: spec.target,
            labels: spec.labels,
            contacts: spec.contacts,
            kind: TestKind::Shell,
        };
        return vec![test]; // i.e; "just run it"
    }

    /// Evaluates the test based solely on its exit code.
    pub fn evaluate(result: &mut Child) -> bool {
        result.wait().unwrap().success() // shouldn't block, it already ran
    }
}

/// Refer to https://buck.build/files-and-dirs/buckconfig.html#test.external_runner
///
/// A JSON spec needs at least these fields, any others will be ignored.
#[derive(Clone, Debug, Deserialize)]
pub struct TestSpec {
    /// The buck target URI of the test rule.
    pub target: String,

    /// The type of the test.
    #[serde(rename(deserialize = "type"))]
    kind: TestKind,

    /// Command line that should be used to run the test.
    pub command: Vec<String>,

    /// Environment variables to be defined when running the test.
    pub env: HashMap<String, String>,

    /// Labels that are defined on the test rule.
    pub labels: HashSet<String>,

    /// Contacts that are defined on the test rule.
    pub contacts: HashSet<String>,

    /// Working directory the test should be run from.
    pub cwd: Option<PathBuf>,

    /// Absolute paths to any files required by this test target.
    required_paths: Option<Vec<PathBuf>>,
}

/// Reads test specs from a buck test run description at the given path.
pub fn read<P: AsRef<Path>>(path: P) -> Result<Vec<TestSpec>> {
    let path = path.as_ref();

    let file = File::open(path)
        .with_context(|| format!("Failed to read test specs from {}", path.display()))?;

    let specs: Vec<TestSpec> = serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("Failed to parse JSON spec from {}", path.display()))?;

    return Ok(specs);
}

/// Validates a spec into a (possibly empty) set of runnable tests.
pub fn validate(spec: TestSpec) -> Result<Vec<Test>> {
    for &exclude in EXCLUDED_LABELS {
        if spec.labels.contains(exclude) {
            return Ok(vec![]);
        }
    }

    // we'll collect all validation errors we can find
    let mut error = String::new();

    if spec.command.len() < 1 {
        let msg = format!("    Error: Empty command line\n");
        error.push_str(&msg);
    }

    // if requirements were specified, we better verify they're there
    for req in spec.required_paths.clone().unwrap_or(vec![]) {
        if !req.exists() {
            let msg = format!("    Error: Missing requirement {}\n", req.display());
            error.push_str(&msg);
        }
    }

    if !error.is_empty() {
        bail!("Invalid context for test target {}\n{}", spec.target, error);
    }

    // dispatch on kind for further processing
    let tests = match spec.kind {
        TestKind::Pyunit => pyunit::list_tests(spec),
        TestKind::Rust => rust::list_tests(spec),
        TestKind::Shell => shell::list_tests(spec),
    };

    return Ok(tests);
}

/// Builds a new command from the given arguments.
pub fn make_command(
    cmd: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
) -> Command {
    // refer to https://doc.rust-lang.org/std/process/struct.Command.html#impl
    // notably, a Command will inherit the current env, working dir and stdin/out/err
    let mut command = Command::new(cmd);
    command.args(args);

    // avoid leaking the environment
    command.env_clear();
    command.envs(env);

    // if no path was specified, use the current one
    let cwd = match cwd {
        None => env::current_dir().unwrap(),
        Some(path) => path,
    };
    command.current_dir(cwd);

    // no stdin, but we'll want stdout and stderr afterwards
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    return command;
}
