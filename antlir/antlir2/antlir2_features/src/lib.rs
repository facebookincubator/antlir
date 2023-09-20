/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::io::Seek;
use std::process::Command;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

pub mod stat;
pub mod types;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature {
    #[serde(deserialize_with = "Label::deserialize_owned")]
    pub label: Label,
    pub feature_type: String,
    pub data: serde_json::Value,
    pub run_info: Vec<String>,
}

impl Feature {
    /// Create a Command that will run this feature implementation process with the data passed as a cli arg
    pub fn base_cmd(&self) -> Command {
        let mut run_info = self.run_info.iter();
        let mut cmd = Command::new(
            run_info
                .next()
                .expect("run_info will always have >=1 element"),
        );
        let opts = memfd::MemfdOptions::default().close_on_exec(false);
        let mfd = opts
            .create("stdin")
            .expect("failed to create memfd for stdin");
        serde_json::to_writer(&mut mfd.as_file(), &self.data)
            .expect("serde_json::Value reserialization will never fail");
        mfd.as_file().rewind().expect("failed to rewind memfd");
        cmd.args(run_info).stdin(mfd.into_file());
        cmd
    }
}

impl PartialOrd for Feature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Feature {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.label.cmp(&other.label) {
            Ordering::Equal => match self.feature_type.cmp(&other.feature_type) {
                Ordering::Equal => serde_json::to_string(&self.data)
                    .expect("failed to serialize")
                    .cmp(&serde_json::to_string(&other.data).expect("failed to serialize")),
                ord => ord,
            },
            ord => ord,
        }
    }
}
