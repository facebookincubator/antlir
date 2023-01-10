/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::process::ExitCode;

use buck_label::Label;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    includes: Vec<Label<'static>>,
    #[clap(long)]
    excludes: Vec<Label<'static>>,
    #[clap(long)]
    actual: Vec<Label<'static>>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let includes: BTreeSet<_> = args
        .includes
        .into_iter()
        .map(|l| l.as_unconfigured())
        .collect();
    let excludes: BTreeSet<_> = args
        .excludes
        .into_iter()
        .map(|l| l.as_unconfigured())
        .collect();
    let actual: BTreeSet<_> = args
        .actual
        .into_iter()
        .map(|l| l.as_unconfigured())
        .collect();
    if !actual.is_superset(&includes) {
        for missing in includes.difference(&actual) {
            println!("missing expected dep {missing}");
        }
        return ExitCode::FAILURE;
    }
    if !actual.is_disjoint(&excludes) {
        for included in actual.intersection(&excludes) {
            println!("excluded dep is still present {included}")
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
