/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use anyhow::{Context, Result};
use itertools::Itertools;
use mysql_async::prelude::*;
use mysql_async::Conn;
use rayon::iter::*;
use structopt::{clap, StructOpt};

// we declare all modules here so that they may refer to each other using `super::<mod>`
mod buck_test;
mod pyunit;
mod rust;

use buck_test::{Test, TestResult, TestStatus};

#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// Path to JSON-encoded test descriptions. Passed in by buck test
    #[structopt(long = "buck-test-info")]
    spec: PathBuf,

    /// Lists unit tests and exits without running them
    #[structopt(long, short)]
    list: bool,

    /// Path to generated test report in JUnit XML format
    #[structopt(long = "xml")]
    report: Option<PathBuf>,

    /// Connection string of the DB used in stateful test runs. Defaults to read-only
    #[structopt(long = "db")]
    conn: Option<String>,

    /// Revision identifier that enables writing results to the test DB
    #[structopt(long = "commit", requires("conn"))]
    revision: Option<String>,

    /// Forces auto-disabled tests to run
    #[structopt(long)]
    run_disabled: bool,

    /// Maximum number of times a failing unit test will be retried
    #[structopt(long = "max-retries", short = "r", default_value = "0")]
    retries: u32,

    /// Maximum number of concurrent tests. Passed in by buck test
    #[structopt(long = "jobs", default_value = "1")]
    threads: usize,

    /// Warns on any further options for forward compatibility with buck
    #[structopt(hidden = true)]
    ignored: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
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
            println!("{}#{}", test.target, test.unit);
        }
        exit(0);
    }

    // connect to DB when provided. we handle errors manually to avoid leaking credentials
    let db = match options.conn {
        None => None,
        Some(ref uri) => match Conn::from_url(uri).await {
            Ok(connection) => Some(connection),
            Err(_) => panic!("Couldn't connect to specified test DB"),
        },
    };
    let (db, disabled) = query_disabled(db).await;

    // run tests in parallel (retries share the same thread)
    let total = tests.len();
    eprintln!("Found {} tests...", total);
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads)
        .build_global();
    let tests: Vec<TestResult> = tests
        .into_par_iter()
        .map(|test| {
            let should_run = !disabled.contains(&(test.target.clone(), test.unit.clone()))
                || options.run_disabled;

            let test = match should_run {
                true => test.run(options.retries),
                false => TestResult {
                    target: test.target,
                    unit: test.unit,
                    status: TestStatus::Disabled,
                    attempts: 0,
                    duration: Duration::ZERO,
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    contacts: test.contacts,
                },
            };

            let name = format!("{}#{}", test.target, test.unit);
            match test.status {
                TestStatus::Pass => {
                    print!("[OK] {} ({} ms)", name, test.duration.as_millis());
                    if test.attempts > 1 {
                        print!(" ({} attempts needed)\n", test.attempts);
                    } else {
                        print!("\n");
                    }
                }
                TestStatus::Fail => {
                    println!("[FAIL] {} ({} ms)", name, test.duration.as_millis());
                }
                TestStatus::Disabled => {
                    println!("[DISABLED] {}", name);
                }
            }
            return test;
        })
        .collect();

    // collect and print results
    let (passed, errors, disabled) = tests.iter().fold(
        (0, Vec::new(), 0),
        |(passed, mut errors, disabled), test| match test.status {
            TestStatus::Pass => (passed + 1, errors, disabled),
            TestStatus::Disabled => (passed, errors, disabled + 1),
            TestStatus::Fail => {
                let mut message = format!(
                    "\nTest {}#{} failed after {} unsuccessful attempts:\n",
                    test.target, test.unit, test.attempts
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
                (passed, errors, disabled)
            }
        },
    );
    let failed = errors.len();
    for error in errors {
        eprintln!("{}", error);
    }
    println!(
        "Out of {} tests, {} passed, {} failed, {} are disabled",
        total, passed, failed, disabled
    );

    // generate outputs
    if let Some(path) = options.report {
        report(&tests, path)?;
    }
    if let Some(revision) = options.revision {
        commit_test_results(db, revision, &tests).await?;
    }

    exit(failed as i32);
}

// Refer to https://llg.cubic.org/docs/junit/
fn report<P: AsRef<Path>>(tests: &[TestResult], path: P) -> Result<()> {
    let path = path.as_ref();
    let file = File::create(&path).with_context(|| {
        format!(
            "Couldn't generate report at specified path {}",
            path.display()
        )
    })?;
    let mut xml = BufWriter::new(&file);

    let failures = tests
        .iter()
        .filter(|test| match test.status {
            TestStatus::Fail => true,
            _ => false,
        })
        .count();
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
        .into_group_map_by(|test| test.target.clone());
    for (target, cases) in suites {
        let failures = cases
            .iter()
            .filter(|test| match test.status {
                TestStatus::Fail => true,
                _ => false,
            })
            .count();
        let skipped = cases.iter().filter(|test| test.attempts == 0).count();
        writeln!(
            xml,
            r#"  <testsuite name="{}" tests="{}" failures="{}" skipped="{}">"#,
            target,
            cases.len(),
            failures,
            skipped
        )?;

        for test in cases {
            write!(
                xml,
                r#"    <testcase classname="{}" name="{}" time="{}""#,
                &test.target,
                &test.unit,
                test.duration.as_millis() as f32 / 1e3
            )?;
            match test.status {
                TestStatus::Disabled | TestStatus::Pass => {
                    writeln!(xml, " />")?;
                }
                TestStatus::Fail => {
                    writeln!(xml, r#">"#)?;
                    writeln!(
                        xml,
                        r#"      <failure>Test failed after {} unsuccessful attempts</failure>"#,
                        test.attempts
                    )?;
                    writeln!(
                        xml,
                        r#"      <system-out>{}</system-out>"#,
                        xml_escape_text(&test.stdout)
                    )?;
                    writeln!(
                        xml,
                        r#"      <system-err>{}</system-err>"#,
                        xml_escape_text(&test.stderr)
                    )?;
                    writeln!(xml, r#"    </testcase>"#)?;
                }
            }
        }

        writeln!(xml, r#"  </testsuite>"#)?;
    }

    writeln!(xml, r#"</testsuites>"#)?;

    eprintln!("Test report written to {}", path.display());
    return Ok(());
}

fn xml_escape_text(unescaped: &String) -> String {
    return unescaped.replace("<", "&lt;").replace("&", "&amp;");
}

async fn query_disabled(db: Option<Conn>) -> (Option<Conn>, HashSet<(String, String)>) {
    match db {
        None => (db, HashSet::new()),
        Some(db) => {
            let result = db
                .prep_exec("SELECT target, test FROM tests WHERE disabled = true", ())
                .await
                .unwrap();
            let (db, disabled) = result.map_and_drop(mysql_async::from_row).await.unwrap();
            return (Some(db), disabled.into_iter().collect());
        }
    }
}

async fn commit_test_results(
    db: Option<Conn>,
    revision: String,
    tests: &[TestResult],
) -> Result<Option<Conn>> {
    match db {
        None => Ok(db),
        Some(db) => {
            // NOTE: INSERT IGNORE and ON DUPLICATE are MySQL-specific ways to upsert
            let insert_target = "INSERT IGNORE INTO targets (target)
                VALUES (:target)";

            let insert_test = "INSERT IGNORE INTO tests (target, test, disabled)
                VALUES (:target, :test, false)";

            let insert_result = "INSERT IGNORE INTO results (revision, target, test, passed)
                VALUES (:revision, :target, :test, :passed)";

            let select_last_3 = "
                SELECT test.passed as passed
                FROM results test, runs run
                WHERE test.target = :target
                AND test.test = :test
                AND run.revision = test.revision
                ORDER BY run.time DESC
                LIMIT 3";

            let update_disabled = "UPDATE tests
                SET disabled = :disabled
                WHERE target = :target
                AND test = :test";

            let mut db = db
                .drop_exec(
                    "INSERT INTO runs (revision)
                VALUES (:revision)
                ON DUPLICATE KEY UPDATE time = CURRENT_TIMESTAMP",
                    params! {
                        "revision" => &revision,
                    },
                )
                .await?;

            for test in tests {
                let passed = match test.status {
                    TestStatus::Pass => true,
                    _ => false,
                };

                db = db
                    .drop_exec(
                        insert_target,
                        params! {
                            "target" => &test.target,
                        },
                    )
                    .await?;
                db = db
                    .drop_exec(
                        insert_test,
                        params! {
                            "target" => &test.target,
                            "test" => &test.unit,
                        },
                    )
                    .await?;
                db = db
                    .drop_exec(
                        insert_result,
                        params! {
                            "revision" => &revision,
                            "target" => &test.target,
                            "test" => &test.unit,
                            "passed" => passed,
                        },
                    )
                    .await?;

                // auto-disable tests which, after this run, have failed 3 or more times in a row
                let result = db
                    .prep_exec(
                        select_last_3,
                        params! {
                            "target" => &test.target,
                            "test" => &test.unit
                        },
                    )
                    .await?;
                let (db_, fails) = result.map_and_drop(mysql_async::from_row).await?;
                db = db_;
                let disabled = fails.into_iter().filter(|passed: &bool| !passed).count() >= 3;
                db = db
                    .drop_exec(
                        update_disabled,
                        params! {
                            "target" => &test.target,
                            "test" => &test.unit,
                            "disabled" => disabled
                        },
                    )
                    .await?;
            }

            Ok(Some(db))
        }
    }
}
