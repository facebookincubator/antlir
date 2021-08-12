use std::path::PathBuf;
use std::process::exit;

use anyhow::Result;
use structopt::{clap, StructOpt};

// we declare all modules here so that they may refer to each other using `super::<mod>`
mod buck_test;
mod pyunit;
mod rust;

use buck_test::Test;

#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// Path to JSON-encoded test descriptions. Passed in by buck test
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

    // validate and collect tests
    let specs = buck_test::read(&options.spec)?;
    let tests: Vec<Test> = specs
        .into_iter()
        .map(|spec| buck_test::validate(spec).unwrap())
        .flatten()
        .collect();

    // run all tests
    let retcode = buck_test::run_all(tests, options.threads);
    exit(retcode);
}
