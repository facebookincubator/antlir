use std::path::PathBuf;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::process;
use std::process::Stdio;
use std::env;

use structopt::{clap, StructOpt};
use serde::Deserialize;
use serde_json;
use anyhow::{Result, Context, Error};


// NOTE: all Optional<_> or defaulted fields are not documented in buck


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
    type_: TestType,

    /// Command line that should be used to run the test.
    command: Vec<String>,

    /// Environment variables to be defined when running the test.
    env: HashMap<String, String>,

    /// Labels that are defined on the test rule.
    labels: Vec<String>,

    /// Contacts that are defined on the test rule.
    contacts: Vec<String>,

    /// Working directory the test should be run from.
    cwd: Option<PathBuf>,

    /// Absolute paths to any files required for this test to run.
    required_paths: Option<Vec<PathBuf>>,

    /// Tuples of required (<test coverage percentage>, <files...>)
    needed_coverage: Option<Vec<(f32, Vec<PathBuf>)>>,
}

/// This is the actual test structure used internally.
#[derive(Debug)]
struct Test {
    target: String,
    command: process::Command,
}

// validates a spec into an actual test
fn validate(spec: TestSpec) -> Result<Test> {
    // we'll collect all validation errors we can find
    let mut err = String::new();

    if spec.command.len() < 1 {
        let msg = format!("Error: Empty command line\n");
        err.push_str(&msg);
    }

    // if requirements were specified, we better verify they're there
    for req in spec.required_paths.unwrap_or(vec![]) {
        if !req.exists() {
            let msg = format!("Error: Missing requirement {}\n", req.display());
            err.push_str(&msg);
        }
    }

    if !err.is_empty() {
        return Err(Error::msg(err));

    // refer to https://doc.rust-lang.org/std/process/struct.Command.html#impl
    // notably, a Command will inherit the current env, working dir and stdin/out/err
    } else {
        let mut command = process::Command::new(&spec.command[0]);
        if spec.command.len() > 1 {
            command.args(&spec.command[1..]);
        }

        // avoid leaking the environment
        command.env_clear();
        command.envs(spec.env);

        // if no path was specified, use the current one
        let dir = match spec.cwd {
            None => env::current_dir().unwrap(),
            Some(path) => path
        };
        command.current_dir(dir);

        // no stdin, but we may use stdout and stderr afterwards
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        return Ok(Test {
            target: spec.target,
            command: command,
        });
    }
}

fn run_all(specs: Vec<TestSpec>, threads: u32) -> Result<()> {
    let tests: Vec<Result<Test>> = specs.into_iter().map(validate).collect();
    println!("{:?}", tests);
    Ok(())
}


#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// [BUCK] JSON file containing test descriptions
    #[structopt(parse(from_os_str), long = "buck-test-info")]
    spec: PathBuf,

    /// [BUCK] Maximum number of threads used
    #[structopt(long = "jobs", default_value = "1")]
    threads: u32,

    /// [BUCK] Warns on any further options for forward compatibility
    #[structopt(hidden = true)]
    ignored: Vec<String>,
}

fn main() -> Result<()> {
    // parse command line
    let options = Options::from_args();
    if options.ignored.len() > 0 {
        println!(
            "Warning: Unimplemented options were ignored: {:?}\n",
            options.ignored
        );
    }

    // collect test information
    let file = File::open(&options.spec).with_context(||
        format!("Failed to read test specs from {}", options.spec.display())
    )?;
    let reader = BufReader::new(file);
    let tests: Vec<TestSpec> = serde_json::from_reader(reader).with_context(||
        format!("Failed to parse JSON spec from {}", options.spec.display())
    )?;

    // run them
    run_all(tests, options.threads)
}
