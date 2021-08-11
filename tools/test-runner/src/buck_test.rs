use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use rayon::iter::*;
use serde::Deserialize;

use super::pyunit;

/// Unit test.
#[derive(Debug)]
pub struct Test {
    /// The command used to execute the test.
    pub command: Command,

    /// User-facing identifier for this specific test.
    pub name: String,

    /// Labels/tags associated to this test.
    pub labels: Vec<String>,

    /// Contacts for further information.
    pub contacts: Vec<String>,

    /// Which type of test this is.
    pub kind: TestKind,
}

/// Supported types of tests.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    /// Buck's custom `unittest` runner.
    Pyunit,

    /// Anything else: simply runs a command and checks for non-zero exit code.
    Shell,
}

/// Runs all given tests, with a bound on concurrent processes.
pub fn run_all(tests: Vec<Test>, threads: usize) -> i32 {
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global();

    // run tests in parallel
    let total = tests.len();
    eprintln!("Running {} tests...", total);
    let tests: Vec<(Test, Child)> = tests
        .into_par_iter()
        .map(|mut test| {
            let mut child = test.command.spawn().unwrap();
            let _ = child.wait().unwrap(); // wait on each thread
            return (test, child);
        })
        .collect();

    // collect results, evaluating them based on what kind it is
    let mut passed = 0;
    let mut errors: Vec<String> = Vec::new();
    for (test, mut result) in tests {
        let pass = match test.kind {
            TestKind::Shell => shell::evaluate(&mut result),
            TestKind::Pyunit => pyunit::evaluate(&mut result),
        };
        if pass {
            eprintln!("[PASS] {}", test.name);
            passed += 1;
        } else {
            eprintln!("[FAIL] {}", test.name);
            let mut message = format!("\nTest {} failed:\n", test.name);
            let stderr = result.stderr.take().unwrap();
            for line in BufReader::new(stderr).lines() {
                let line = format!("    {}\n", line.unwrap());
                message.push_str(&line);
            }
            errors.push(message);
        }
    }

    // let the user know of any errors
    for error in errors {
        eprintln!("{}", error);
    }

    // put a summary in the output, as well as in this runner's return code
    let percent = 100.0 * passed as f32 / total as f32;
    println!("{:.2}% tests passed ({} out of {})", percent, passed, total);
    let failing = (total - passed) as i32;
    return failing;
}

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
    #[serde(rename(deserialize = "type"))]
    kind: TestKind,

    /// Command line that should be used to run the test.
    pub command: Vec<String>,

    /// Environment variables to be defined when running the test.
    pub env: HashMap<String, String>,

    /// Labels that are defined on the test rule.
    pub labels: Vec<String>,

    /// Contacts that are defined on the test rule.
    pub contacts: Vec<String>,

    /// Working directory the test should be run from.
    pub cwd: Option<PathBuf>,

    /// Absolute paths to any files required by this test target.
    required_paths: Option<Vec<PathBuf>>,
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
        TestKind::Shell => shell::validate(spec),
        TestKind::Pyunit => pyunit::validate(spec),
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
