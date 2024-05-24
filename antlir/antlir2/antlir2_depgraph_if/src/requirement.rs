/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;

use serde::Deserialize;
use serde::Serialize;

use crate::item::ItemKey;
use crate::Validator;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Requirement {
    pub key: ItemKey,
    pub validator: Validator,
    /// This [Requirement] necessitates ordered running of the features
    /// involved. If false, the compiler is free to run the features in any
    /// order.
    pub ordered: bool,
}

impl Requirement {
    /// Hard build dependencies (eg: parent dir exists before install) should
    /// use this function. The compiler will not attempt to build the feature
    /// that has this [Requirement] until the feature that provides it has been
    /// built.
    pub fn ordered(key: ItemKey, validator: Validator) -> Self {
        Self {
            key,
            validator,
            ordered: true,
        }
    }

    /// Logical requirements (eg user's home directory exists) should use this
    /// function. The compiler is free to build the feature that has this
    /// [Requirement] before the feature that provides it, which is useful for
    /// avoiding ordering cycles for purely logical "this has to happen by the
    /// time the layer is done" requirements.
    pub fn unordered(key: ItemKey, validator: Validator) -> Self {
        Self {
            key,
            validator,
            ordered: false,
        }
    }
}
