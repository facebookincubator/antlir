/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use systemd::analyze::verify::verify;
use systemd::analyze::verify::Problem;
use systemd::UnitFileState;
use systemd::UnitName;

static EXPECTATIONS: &'static str = include_str!("expectations.toml");

#[derive(Debug, Deserialize)]
struct Expectations {
    #[serde(flatten)]
    units: BTreeMap<UnitName, UnitExpectation>,
    problem: BTreeMap<UnitName, BTreeSet<Problem>>,
}

#[derive(Debug, Deserialize)]
struct UnitExpectation {
    state: UnitFileState,
}

#[derive(Debug, Deserialize)]
struct ListedUnit {
    unit_file: UnitName,
    state: UnitFileState,
}

fn list_unit_files() -> Result<Vec<ListedUnit>> {
    let out = Command::new("systemctl")
        .arg("list-unit-files")
        .arg("--output=json")
        .arg("--all")
        .output()
        .context("failed to list unit files")?;
    ensure!(out.status.success(), "systemctl list-unit-files failed");
    serde_json::from_slice(&out.stdout).context("failed to deserialize list-unit-files")
}

#[test]
fn unit_expectations() -> Result<()> {
    let expectations: Expectations = toml::from_str(EXPECTATIONS)?;

    let unit_file_states: BTreeMap<UnitName, UnitFileState> = list_unit_files()?
        .into_iter()
        .map(|u| (u.unit_file, u.state))
        .collect();
    // also confirm that we checked all the units we had expectations for
    for (unit, expect) in expectations.units {
        match unit_file_states.get(&unit) {
            Some(state) => {
                assert_eq!(
                    expect.state, *state,
                    "{} should have been {}, but was {}",
                    unit, expect.state, state
                );
            }
            None => {
                panic!("{} is missing", unit);
            }
        };
    }
    Ok(())
}

#[test]
fn verify_all_units() -> Result<()> {
    let expectations: Expectations = toml::from_str(EXPECTATIONS)?;

    let units: BTreeSet<_> = list_unit_files()?
        .into_iter()
        .filter_map(|u| match u.state {
            UnitFileState::Masked | UnitFileState::MaskedRuntime | UnitFileState::Disabled => None,
            _ => Some(u.unit_file),
        })
        .collect();

    let problems = verify(units)?;

    assert_eq!(problems, expectations.problem);
    Ok(())
}
