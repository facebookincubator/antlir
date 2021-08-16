use std::path::PathBuf;
use std::process::exit;

use anyhow::Result;
use rayon::iter::*;
use structopt::{clap, StructOpt};

// we declare all modules here so that they may refer to each other using `super::<mod>`
mod buck_test;
mod pyunit;
mod rust;

use buck_test::{Test, TestResult};

#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// Path to JSON-encoded test descriptions. Passed in by buck test
    #[structopt(long = "buck-test-info")]
    spec: PathBuf,

    /// Lists all unit tests and exits without running them
    #[structopt(long)]
    list: bool,

    /// Path to generated test report in JUnit XML format
    #[structopt(long = "xml")]
    report: Option<PathBuf>,

    /// Maximum number of times a failing unit test will be retried
    #[structopt(long = "max-retries", default_value = "0")]
    retries: u32,

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

    // validate and collect tests which are not auto-excluded
    let specs = buck_test::read(&options.spec)?;
    let tests: Vec<Test> = specs
        .into_iter()
        .map(|spec| buck_test::validate(spec).unwrap())
        .flatten()
        .collect();

    // don't run anything when just listing
    if options.list {
        for test in tests {
            println!("{}", test.name);
        }
        exit(0);
    }

    // run tests in parallel (retries share the same thread)
    let total = tests.len();
    eprintln!("Running {} tests...", total);
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads)
        .build_global();
    let mut tests: Vec<TestResult> = tests
        .into_par_iter()
        .map(|test| {
            let test = buck_test::run(test, options.retries);
            if test.passed {
                print!("[OK] {} ({} ms)", test.name, test.duration.as_millis());
                if test.attempts > 1 {
                    print!(" ({} attempts needed)\n", test.attempts);
                } else {
                    print!("\n");
                }
            } else {
                println!("[FAIL] {} ({} ms)", test.name, test.duration.as_millis());
            }
            return test;
        })
        .collect();

    // collect and print results
    let mut passed = 0;
    let mut errors: Vec<String> = Vec::new();
    for test in tests.iter_mut() {
        if test.passed {
            passed += 1;
        } else {
            let mut message = format!(
                "\nTest {} failed after {} unsuccessful attempts:\n",
                test.name, test.attempts
            );
            for line in test.stderr.split("\n") {
                let line = format!("    {}\n", line);
                message.push_str(&line);
            }
            if test.contacts.len() > 0 {
                let contacts = format!("Please report this to {:?}\n", test.contacts);
                message.push_str(&contacts);
            }
            errors.push(message);
        }
    }
    for error in errors {
        eprintln!("{}", error);
    }
    let failed = total - passed;
    let percent = 100.0 * passed as f32 / total as f32;
    println!(
        "{:.2}% tests passed ({} out of {})",
        percent, passed, total
    );

    exit(failed as i32);
}
