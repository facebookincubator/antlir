/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

pub mod clone;
pub mod ensure_dir_exists;
pub mod extract;
#[cfg(facebook)]
pub mod facebook;
pub mod genrule;
pub mod install;
pub mod meta_kv;
pub mod mount;
pub mod remove;
pub mod requires;
pub mod rpms;
pub mod stat;
pub mod symlink;
pub mod tarball;
pub mod types;
pub mod usergroup;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature<'a> {
    #[serde(borrow, rename = "__label")]
    pub label: Label<'a>,
    #[serde(flatten)]
    pub data: Data<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(
    rename_all = "snake_case",
    tag = "__feature_type",
    bound(deserialize = "'de: 'a")
)]
pub enum Data<'a> {
    Clone(clone::Clone<'a>),
    EnsureDirSymlink(symlink::Symlink<'a>),
    EnsureDirExists(ensure_dir_exists::EnsureDirExists<'a>),
    EnsureFileSymlink(symlink::Symlink<'a>),
    Extract(extract::Extract<'a>),
    Genrule(genrule::Genrule<'a>),
    Group(usergroup::Group<'a>),
    Install(install::Install<'a>),
    Meta(meta_kv::Meta<'a>),
    Mount(mount::Mount<'a>),
    Remove(remove::Remove<'a>),
    Requires(requires::Requires<'a>),
    Rpm(rpms::Rpm<'a>),
    Tarball(tarball::Tarball<'a>),
    User(usergroup::User<'a>),
    UserMod(usergroup::UserMod<'a>),
    #[cfg(facebook)]
    #[serde(rename = "facebook/chef_solo")]
    ChefSolo(facebook::ChefSolo<'a>),
}
