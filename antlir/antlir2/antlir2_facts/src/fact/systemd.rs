/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_systemd::UnitFile;

use super::Fact;

impl<'a> Fact<'a, '_> for UnitFile {
    type Key = &'a str;

    fn key(&'a self) -> Self::Key {
        self.name()
    }
}
