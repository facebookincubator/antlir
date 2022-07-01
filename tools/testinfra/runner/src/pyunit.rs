/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::process::Child;

use super::buck_test::make_command;
use super::buck_test::shell;
use super::buck_test::Test;
use super::buck_test::TestKind;
use super::buck_test::TestSpec;

pub fn list_tests(spec: TestSpec) -> Vec<Test> {
    let base_command = || {
        make_command(
            spec.command[0].clone(),
            spec.command[1..].to_vec(),
            spec.env.clone(),
            spec.cwd.clone(),
        )
    };

    // list all unit tests in the format:
    //     <module>#<function1>
    //     <module>#<function2>
    //     ...
    let mut list_tests = base_command();
    let mut list_tests = list_tests
        .arg("--list-tests")
        .arg("--list-format=buck")
        .spawn()
        .unwrap();
    let status = list_tests.wait().unwrap();
    let mut stdout = list_tests.stdout.unwrap();
    if !status.success() {
        let mut stderr = list_tests.stderr.unwrap();
        let _ = io::copy(&mut stdout, &mut io::stderr());
        let _ = io::copy(&mut stderr, &mut io::stderr());
        eprint!("\n");
        panic!("Failed to list tests from {}", spec.target);
    }

    // parse those into a set of individual tests
    let mut tests = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.unwrap();
        let line: Vec<&str> = line.split("#").collect();
        let module = line[0];
        let function = line[1];
        let unit = module.to_string() + "." + function;

        // make a command to run only this unit test
        let mut unit_command = base_command();
        unit_command.arg(unit);
        tests.push(Test {
            command: unit_command,
            name: spec.target.clone() + "#" + function,
            labels: spec.labels.clone(),
            contacts: spec.contacts.clone(),
            kind: TestKind::Pyunit,
        });
    }

    return tests;
}

pub fn evaluate(result: &mut Child) -> bool {
    return shell::evaluate(result);
}
