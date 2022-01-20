/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};

const BUCK_ABS_PATH: &str = "/usr/local/bin/buck";

pub fn buck_query<T: for<'de> Deserialize<'de>>(query: &str, attrs: bool) -> Result<T> {
    let mut args = vec![];
    if attrs {
        args.push("--output-all-attributes");
    }

    let cmd = if Path::new(BUCK_ABS_PATH).exists() {
        BUCK_ABS_PATH
    } else {
        "buck"
    };

    let proc = Command::new(cmd)
        .arg("query")
        .arg("--json")
        .args(args)
        .arg(query)
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to spawn 'buck query'")?;
    serde_json::from_reader(proc.stdout.unwrap()).context("failed to parse query output")
}
