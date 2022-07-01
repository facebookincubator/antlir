/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;

use anyhow::Context;
use anyhow::Result;
use itertools::Itertools;
use rayon::iter::*;
use structopt::clap;
use structopt::StructOpt;

// we declare all modules here so that they may refer to each other using `super::<mod>`
mod buck_test;
mod pyunit;
mod rust;

use buck_test::Test;
use buck_test::TestResult;

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
    let tests: Vec<Test> = buck_test::read(&options.spec)?
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
            let test = test.run(options.retries);
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
    println!(
        "Out of {} tests, {} passed, {} failed",
        total, passed, failed
    );

    // generate test report
    match options.report {
        None => {}
        Some(path) => report(tests, path)?,
    }

    exit(failed as i32);
}

// Refer to https://llg.cubic.org/docs/junit/
fn report<P: AsRef<Path>>(tests: Vec<TestResult>, path: P) -> Result<()> {
    let path = path.as_ref();
    let file = File::create(&path).with_context(|| {
        format!(
            "Couldn't generate report at specified path {}",
            path.display()
        )
    })?;
    let mut xml = BufWriter::new(&file);

    let failures = tests.iter().filter(|test| !test.passed).count();
    writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        xml,
        r#"<testsuites tests="{}" failures="{}">"#,
        tests.len(),
        failures
    )?;

    // we group unit tests from the same buck target as a JUnit "testsuite"
    let suites = tests
        .into_iter()
        .map(|test| {
            let name: Vec<&str> = test.name.split("#").collect();
            let target = name[0].to_owned();
            let unit = if name.len() > 1 {
                name[1].to_owned()
            } else {
                target.clone()
            };
            return (target, unit, test);
        })
        .into_group_map_by(|(target, _, _)| target.to_owned());
    for (target, cases) in suites {
        let failures = cases.iter().filter(|(_, _, test)| !test.passed).count();
        writeln!(
            xml,
            r#"  <testsuite name="{}" tests="{}" failures="{}">"#,
            target,
            cases.len(),
            failures
        )?;

        for (target, unit, test) in cases {
            write!(
                xml,
                r#"    <testcase classname="{}" name="{}" time="{}""#,
                target,
                unit,
                test.duration.as_millis() as f32 / 1e3
            )?;
            if test.passed {
                writeln!(xml, " />")?;
            } else {
                writeln!(xml, r#">"#)?;
                writeln!(
                    xml,
                    r#"      <failure>Test failed after {} unsuccessful attempts</failure>"#,
                    test.attempts
                )?;
                writeln!(
                    xml,
                    r#"      <system-out>{}</system-out>"#,
                    xml_escape_text(test.stdout)
                )?;
                writeln!(
                    xml,
                    r#"      <system-err>{}</system-err>"#,
                    xml_escape_text(test.stderr)
                )?;
                writeln!(xml, r#"    </testcase>"#)?;
            }
        }

        writeln!(xml, r#"  </testsuite>"#)?;
    }

    writeln!(xml, r#"</testsuites>"#)?;

    eprintln!("Test report written to {}", path.display());
    return Ok(());
}

fn xml_escape_text(unescaped: String) -> String {
    return unescaped.replace("<", "&lt;").replace("&", "&amp;");
}
