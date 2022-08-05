/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use systemd::analyze::verify::verify;
use systemd::analyze::verify::Problem;
use systemd::Systemd;
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

#[tokio::test]
async fn unit_expectations() -> Result<()> {
    let expectations: Expectations = toml::from_str(EXPECTATIONS)?;

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let sd = Systemd::connect(log.clone()).await?;

    let unit_file_states: BTreeMap<UnitName, UnitFileState> = sd
        .list_unit_files()
        .await?
        .into_iter()
        .map(|(file, state)| (file.file_name().unwrap().to_str().unwrap().into(), state))
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

#[tokio::test]
async fn verify_all_units() -> Result<()> {
    let expectations: Expectations = toml::from_str(EXPECTATIONS)?;

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let sd = Systemd::connect(log.clone()).await?;
    let paths: Vec<PathBuf> = sd
        .list_unit_files()
        .await?
        .into_iter()
        .filter_map(|(path, state)| match state {
            UnitFileState::Masked | UnitFileState::MaskedRuntime | UnitFileState::Disabled => None,
            _ => Some(path.into()),
        })
        .collect();

    let problems = verify(&paths)?;

    assert_eq!(problems, expectations.problem);
    Ok(())
}
