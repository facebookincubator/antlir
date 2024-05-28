/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_features::Feature;
use serde::Deserialize;
use serde::Serialize;

pub mod item;
mod requirement;
mod validator;

use item::Item;
pub use requirement::Requirement;
pub use validator::Validator;

pub trait RequiresProvides {
    fn provides(&self) -> std::result::Result<Vec<Item>, String>;
    fn requires(&self) -> std::result::Result<Vec<Requirement>, String>;
}

static_assertions::assert_obj_safe!(RequiresProvides);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzedFeature {
    feature: Feature,
    requires: Vec<Requirement>,
    provides: Vec<Item>,
}

impl AnalyzedFeature {
    pub fn new(feature: Feature, requires: Vec<Requirement>, provides: Vec<Item>) -> Self {
        Self {
            feature,
            requires,
            provides,
        }
    }

    pub fn feature(&self) -> &Feature {
        &self.feature
    }

    pub fn requires(&self) -> &[Requirement] {
        self.requires.as_slice()
    }

    pub fn provides(&self) -> &[Item] {
        self.provides.as_slice()
    }
}
