/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use features::Data;
use features::Feature;
use serde::Deserialize;
use serde::Serialize;
use strum_macros::Display;
use strum_macros::EnumIter;

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    EnumIter,
    Display,
    Deserialize,
    Serialize
)]
pub enum Phase {
    /// Set up the layer for the build
    Init,
    /// Remove or install any OS installed packages
    OsPackage,
    /// Install new things into the image. The vast majority of image features
    /// go here and are topologically sorted according to dependency edges.
    Install,
    /// Validate that any virtual dependencies are met
    Validate,
    /// End of image build, this is where dynamic provides go after the build is
    /// complete.
    End,
}

impl Phase {
    pub fn for_feature(feature: &Feature) -> Self {
        match &feature.data {
            Data::ParentLayer(_) => Self::Init,
            Data::ReceiveSendstream(_) => Self::Init,
            Data::Rpm(_) => Self::OsPackage,
            #[cfg(facebook)]
            Data::ChefSolo(_) => Self::OsPackage,
            Data::Requires(_) => Self::Validate,
            _ => Self::Install,
        }
    }
}
