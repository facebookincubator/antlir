use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json;
use structopt::{clap, StructOpt};

#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// JSON file containing test descriptions. Passed in by buck test
    #[structopt(long = "buck-test-info")]
    spec: PathBuf,

    /// Maximum number of concurrent tests. Passed in by buck test
    #[structopt(long = "jobs", default_value = "1")]
    threads: u32,

    /// Warns on any further options for forward compatibility with buck test
    #[structopt(hidden = true)]
    ignored: Vec<String>,
}

fn main() -> Result<()> {
    // parse command line
    let options = Options::from_args();
    if options.ignored.len() > 0 {
        eprintln!(
            "Warning: Unimplemented options were ignored: {:?}\n",
            options.ignored
        );
    }

    // collect and validate test information
    let file = File::open(&options.spec)
        .with_context(|| format!("Failed to read test specs from {}", options.spec.display()))?;
    let reader = BufReader::new(file);
    let specs: Vec<TestSpec> = serde_json::from_reader(reader)
        .with_context(|| format!("Failed to parse JSON spec from {}", options.spec.display()))?;
    let tests: Result<Vec<Test>, _> = specs.into_iter().map(validate).collect();
    let tests = tests.with_context(|| "Found an invalid test spec")?;

    // run all tests
    let retcode = run_all(tests, options.threads)?;
    process::exit(retcode);
}

/// This is the actual test structure used internally.
#[derive(Debug)]
struct Test {
    buck_target: String,
    command: Command,
}

fn run_all(tests: Vec<Test>, threads: u32) -> Result<i32> {
    // at this point, we should be able to safely run tests, even if they fail
    let total = tests.len();
    println!("Running {} test targets...", total);
    let mut passed = 0;
    let mut errors: Vec<String> = Vec::new();
    for mut test in tests.into_iter() {
        let target = &test.buck_target;
        let mut child = test.command.spawn()?;
        let status = child.wait()?;
        if status.success() {
            println!("[PASS] {}", target);
            passed += 1;
        } else {
            println!("[FAIL] {} ({})", target, status);
            let mut message = format!("\nTarget {} failed:\n", target);
            let stderr = child.stderr.take().unwrap();
            for line in BufReader::new(stderr).lines() {
                let line = format!("    {}\n", line.unwrap());
                message.push_str(&line);
            }
            errors.push(message);
        }
    }

    // if there were any errors, we let the user know what they were
    for error in errors {
        eprintln!("{}", error);
    }

    // put a summary in the output, as well in this runner's return code
    let percent = 100.0 * passed as f32 / total as f32;
    println!(
        "  Summary: {:.2}% test targets passed ({} out of {})",
        percent, passed, total
    );
    let fail_count = (total - passed) as i32;
    Ok(fail_count)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum TestType {
    Pyunit,
}

// refer to https://buck.build/files-and-dirs/buckconfig.html#test.external_runner
// serde_json will expect AT LEAST these fields to be provided, ignoring the rest
#[derive(Deserialize, Debug)]
struct TestSpec {
    /// The buck target URI of the test rule.
    target: String,

    /// The type of the test.
    #[serde(rename(deserialize = "type"))]
    kind: TestType,

    /// Command line that should be used to run the test.
    command: Vec<String>,

    /// Environment variables to be defined when running the test.
    env: HashMap<String, String>,

    /// Labels that are defined on the test rule. NOTE: not used yet
    labels: Vec<String>,

    /// Contacts that are defined on the test rule. NOTE: not used yet
    contacts: Vec<String>,

    /// Working directory the test should be run from. XXX: not documented
    cwd: Option<PathBuf>,

    /// Absolute paths to any files required for this test to run. XXX: not documented
    required_paths: Option<Vec<PathBuf>>,
}

// validates a spec into an actual test
fn validate(spec: TestSpec) -> Result<Test> {
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
        bail!("Invalid context for running test target {}\n{}", spec.target, err);

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
            buck_target: spec.target,
            command,
        });
    }
}
