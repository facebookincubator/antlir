/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;

pub trait OrderedElement: Send + Debug + Display {
    /// The first id associated with the current item
    fn get_first_id(&self) -> u64;
    /// The last id associated with the current item
    fn get_last_id(&self) -> u64;
    /// Whether the last id is shared or not
    fn is_last_id_shared(&self) -> bool;
}
