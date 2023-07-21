/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::cmp::Ordering;
use std::process::Command;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

pub mod stat;
pub mod types;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature<'a> {
    #[serde(borrow)]
    pub label: Label<'a>,
    pub feature_type: Cow<'a, str>,
    pub data: serde_json::Value,
    pub run_info: Vec<Cow<'a, str>>,
}

impl<'a> Feature<'a> {
    /// Create a Command that will run this feature implementation process with the data passed as a cli arg
    pub fn base_cmd(&self) -> Command {
        let mut run_info = self.run_info.iter().map(Cow::as_ref);
        let feature_json = serde_json::to_string(&self.data)
            .expect("serde_json::Value reserialization will never fail");
        let mut cmd = Command::new(
            run_info
                .next()
                .expect("run_info will always have >=1 element"),
        );
        cmd.args(run_info).arg(feature_json);
        cmd
    }
}

impl<'a> PartialOrd for Feature<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for Feature<'a> {
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
