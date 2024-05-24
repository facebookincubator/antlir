/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
