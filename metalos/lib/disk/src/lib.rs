/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct DiskDevPath(pub PathBuf);
