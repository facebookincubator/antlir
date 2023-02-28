/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use derivative::Derivative;
use serde::Deserialize;
use serde::Serialize;

use crate::usergroup::UserName;

#[derive(Debug, Clone, PartialEq, Eq, Derivative, Deserialize, Serialize)]
#[derivative(PartialOrd, Ord)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Genrule<'a> {
    pub cmd: Vec<Cow<'a, str>>,
    pub user: UserName<'a>,
    pub bind_repo_ro: bool,
    pub boot: bool,
    #[derivative(PartialOrd = "ignore", Ord = "ignore")]
    pub container_opts: container_opts::container_opts_t,
}
