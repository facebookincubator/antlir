/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use cad_stack::ObjectStore;
use cap_std::fs::Dir;

pub(crate) struct StubImpl;

impl crate::Stub for StubImpl {
    fn open() -> Dir {
        let store = ObjectStore::open_rw("/package.cad_stack/store", std::iter::empty::<&str>())
            .expect("failed to open store");
        std::fs::create_dir_all("/extracted").expect("failed to create target dir");
        let target = Dir::open_ambient_dir("/extracted", cap_std::ambient_authority())
            .expect("failed to open target dir");
        cad_stack_fs::extract_root_dir(&store, &target).expect("failed to extract root");
        target
    }
}
