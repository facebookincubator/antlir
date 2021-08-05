use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Result};
use rayon::iter::*;
use serde::Deserialize;

/// Supported kinds of tests.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    /// A.k.a `unittest`, refer to https://docs.python.org/3.6/library/unittest.html
    Pyunit,

    /// Anything else: simply runs a command and checks for non-zero exit code.
    Shell,
}

#[derive(Debug)]
pub struct Test {
    kind: TestKind,
    id: String,
    command: Command,
}

/// Runs all given tests, with a maximum number of concurrent threads.
/// NOTE: does not take into account threads created inside the test itself.
pub fn run_all(tests: Vec<Test>, threads: usize) -> i32 {
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global();

    // run tests in parallel
    let total = tests.len();
    eprintln!("Running {} test targets...", total);
    let tests: Vec<(Test, Child)> = tests
        .into_par_iter()
        .map(|mut test| {
            let mut child = test.command.spawn().unwrap();
            let _ = child.wait().unwrap(); // wait on each thread
            return (test, child);
        })
        .collect();

    // collect results
    let mut passed = 0;
    let mut errors: Vec<String> = Vec::new();
    for (test, mut result) in tests {
        let status = result.wait().unwrap(); // this shouldn't block
        if status.success() {
            eprintln!("[PASS] {}", test.id);
            passed += 1;
        } else {
            eprintln!("[FAIL] {} ({})", test.id, status);
            let mut message = format!("\nTarget {} failed:\n", test.id);
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
    println!(
        "{:.2}% test targets passed ({} out of {})",
        percent, passed, total
    );
    let failing = (total - passed) as i32;
    return failing;
}

/// Refer to https://buck.build/files-and-dirs/buckconfig.html#test.external_runner
// serde_json will require at least these fields, ignoring the rest
#[derive(Deserialize, Debug)]
pub struct TestSpec {
    /// The buck target URI of the test rule.
    target: String,

    /// The type of the test.
    #[serde(rename(deserialize = "type"))]
    kind: TestKind,

    /// Command line that should be used to run the test.
    command: Vec<String>,

    /// Environment variables to be defined when running the test.
    env: HashMap<String, String>,

    /// Labels that are defined on the test rule. NOTE: not used yet
    labels: Vec<String>,

    /// Contacts that are defined on the test rule. NOTE: not used yet
    contacts: Vec<String>,

    /// Working directory the test should be run from.
    cwd: Option<PathBuf>,

    /// Absolute paths to any files required by this test target.
    required_paths: Option<Vec<PathBuf>>,
}

/// Validates a spec into a runnable test.
pub fn validate(spec: TestSpec) -> Result<Test> {
    // we'll collect all validation errors we can find
    let mut err = String::new();

    if spec.command.len() < 1 {
        let msg = format!("    Error: Empty command line\n");
        err.push_str(&msg);
    }

    // if requirements were specified, we better verify they're there
    for req in spec.required_paths.unwrap_or(vec![]) {
        if !req.exists() {
            let msg = format!("    Error: Missing requirement {}\n", req.display());
            err.push_str(&msg);
        }
    }

    if !err.is_empty() {
        bail!(
            "Invalid context for running test target {}\n{}",
            spec.target,
            err
        );

    // refer to https://doc.rust-lang.org/std/process/struct.Command.html#impl
    // notably, a Command will inherit the current env, working dir and stdin/out/err
    } else {
        let mut command = Command::new(&spec.command[0]);
        if spec.command.len() > 1 {
            command.args(&spec.command[1..]);
        }

        // avoid leaking the environment
        command.env_clear();
        command.envs(spec.env);

        // if no path was specified, use the current one
        let dir = match spec.cwd {
            None => env::current_dir()?,
            Some(path) => path,
        };
        command.current_dir(dir);

        // no stdin, but we'll want stdout and stderr afterwards
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        return Ok(Test {
            kind: spec.kind,
            id: spec.target,
            command,
        });
    }
}
