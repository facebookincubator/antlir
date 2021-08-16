use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{ChildStderr, ChildStdout, Command, Stdio};

use anyhow::{bail, Context, Result};
use rayon::iter::*;
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
    Shell,
}

/// Labels which mark buck test targets for automatic (and silent) exclusion.
const EXCLUDED_LABELS: &[&str] = &["disabled", "exclude_test_if_transitive_dep"];

#[derive(Debug)]
struct TestResult {
    test: String,
    pass: bool,
    retries: u32,
    stdout: ChildStdout,
    stderr: ChildStderr,
    contacts: HashSet<String>,
}

/// Runs all given tests, with a bound on concurrent processes.
pub fn run_all(tests: Vec<Test>, threads: usize, retries: u32) -> i32 {
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global();

    // run tests in parallel (retries in the same thread)
    let total = tests.len();
    eprintln!("Running {} tests...", total);
    let tests: Vec<TestResult> = tests
        .into_par_iter()
        .map(|mut test| {
            let mut child = test.command.spawn().unwrap();
            for retry in 0..1 + retries {
                let _ = child.wait();
                let pass = match test.kind {
                    TestKind::Pyunit => pyunit::evaluate(&mut child),
                    TestKind::Rust => rust::evaluate(&mut child),
                    TestKind::Shell => shell::evaluate(&mut child),
                };
                if pass {
                    return TestResult {
                        test: test.name,
                        pass: true,
                        retries: retry,
                        stdout: child.stdout.unwrap(),
                        stderr: child.stderr.unwrap(),
                        contacts: test.contacts,
                    };
                }
            }
            return TestResult {
                test: test.name,
                pass: false,
                retries,
                stdout: child.stdout.unwrap(),
                stderr: child.stderr.unwrap(),
                contacts: test.contacts,
            };
        })
        .collect();

    // collect results, evaluating them based on what kind it is
    let mut passed = 0;
    let mut errors: Vec<String> = Vec::new();
    for result in tests {
        if result.pass {
            print!("[OK] {}", result.test);
            if result.retries > 0 {
                print!(" ({} attempts needed)\n", 1 + result.retries);
            } else {
                print!("\n");
            }
            passed += 1;
        } else {
            println!("[FAIL] {}", result.test);
            let mut message = format!(
                "\nTest {} failed after {} unsuccessful attempts:\n",
                result.test,
                1 + result.retries
            );
            for line in BufReader::new(result.stderr).lines() {
                let line = format!("    {}\n", line.unwrap());
                message.push_str(&line);
            }
            if result.contacts.len() > 0 {
                let contacts = format!("Please report this to {:?}\n", result.contacts);
                message.push_str(&contacts);
            }
            errors.push(message);
        }
    }

    // let the user know of any errors
    for error in errors {
        eprintln!("{}", error);
    }

    // put a summary in the output, as well as in this runner's exit code
    let percent = 100.0 * passed as f32 / total as f32;
    println!("{:.2}% tests passed ({} out of {})", percent, passed, total);
    let failing = (total - passed) as i32;
    return failing;
}

// a.k.a., the defaults
pub mod shell {
    use super::{make_command, Test, TestKind, TestSpec};
    use std::process::Child;

    /// Builds a singleton test from a given spec.
    pub fn validate(spec: TestSpec) -> Vec<Test> {
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

    /// Validates the test based solely on its return value.
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
    #[serde(rename(deserialize = "type"), default = "default_kind")]
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
fn default_kind() -> TestKind {
    TestKind::Shell
}

/// Reads test specs from a buck test run description at the given path.
pub fn read(path: &PathBuf) -> Result<Vec<TestSpec>> {
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
    let tests = match spec.kind.clone() {
        TestKind::Pyunit => pyunit::validate(spec),
        TestKind::Rust => rust::validate(spec),
        TestKind::Shell => shell::validate(spec),
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
