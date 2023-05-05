/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use once_cell::sync::OnceCell;

use crate::types::RuntimeOpts;

static RUNTIME: OnceCell<RuntimeOpts> = OnceCell::new();

/// Get runtime struct. Should only be called after `set_runtime`
#[allow(dead_code)]
pub(crate) fn get_runtime() -> &'static RuntimeOpts {
    RUNTIME
        .get()
        .expect("get_runtime called before initilization")
}

/// Set runtime. Should only be called once.
pub(crate) fn set_runtime(runtime: RuntimeOpts) -> Result<(), RuntimeOpts> {
    RUNTIME.set(runtime)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_set_get() {
        let test = RuntimeOpts {
            qemu_system: "qemu_system".into(),
            qemu_img: "qemu_img".into(),
            firmware: "firmware".into(),
            roms_dir: "roms_dir".into(),
        };

        set_runtime(test.clone()).expect("Failed to set runtime");
        assert_eq!(get_runtime(), &test);
    }
}
