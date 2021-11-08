/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Result;
use systemd::analyze::verify::{verify, Problem};
use systemd::{Systemd, UnitFileState, UnitName};

static EXPECTED_PROBLEMS: &'static str = include_str!("expected-problems.toml");

#[tokio::test]
async fn verify_all_units() -> Result<()> {
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

    let expected_problems: BTreeMap<UnitName, BTreeSet<Problem>> =
        toml::from_str(EXPECTED_PROBLEMS)?;
    assert_eq!(problems, expected_problems);
    Ok(())
}
