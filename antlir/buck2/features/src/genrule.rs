/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use derivative::Derivative;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Derivative, Deserialize, Serialize)]
#[derivative(PartialOrd, Ord)]
pub struct Genrule {
    pub cmd: Vec<String>,
    pub user: String,
    pub bind_repo_ro: bool,
    pub boot: bool,
    #[derivative(PartialOrd = "ignore", Ord = "ignore")]
    pub container_opts: container_opts::container_opts_t,
}
