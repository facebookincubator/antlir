use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::exit;

use anyhow::{Context, Result};
use serde_json;
use structopt::{clap, StructOpt};

mod buck;
use buck::{run_all, validate, Test, TestSpec};

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
    threads: usize,

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
    let retcode = run_all(tests, options.threads);
    exit(retcode);
}
