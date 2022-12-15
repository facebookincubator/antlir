/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::BuckOutSource;

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Sendstream,
    #[serde(rename = "sendstream.v2")]
    SendstreamV2,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ReceiveSendstream {
    pub src: BuckOutSource,
    pub format: Format,
}
