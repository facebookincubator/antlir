/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// antlir macros disallow unused crate dependencies to stop the linter from
// complaining, but this is always added to python modules
use cpython as _;
use pyo3::prelude::*;

mod fs_utils;

/// Register a Rust module as a Python submodule. The Rust module must provide a
/// crate-public function with the same name as the module itself, that uses the
/// #[pymodule] macro as described in the PyO3 docs
/// (https://pyo3.rs/v0.16.4/module.html)
macro_rules! submodule {
    ($module:ident, $py:ident, $parent_module:ident) => {{
        let child_module = PyModule::new($py, stringify!($module))?;
        $module::$module($py, child_module)?;
        $parent_module.add_submodule(child_module)?;

        // Small hack to enable intuitive Python import resolution - for example
        // `import antlir.rust.fs_utils` should work directly as expected,
        // instead of requiring a user to `import antlir.rust` and then use
        // `antlir.rust.fs_utils`.
        // Small note: there is no extra initialization cost to doing this like
        // there might be in pure-Python module imports, since all the
        // initialization is done as part of importing the top level
        // `antlir.rust` module in the first place, so this exists solely to
        // make import mechanics work as expected
        $py.import("sys")?
            .getattr("modules")?
            .set_item(concat!("antlir.rust.", stringify!($module)), child_module)?;
        Ok::<_, pyo3::PyErr>(())
    }};
}

#[pymodule]
fn native(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    submodule!(fs_utils, py, m)?;
    Ok(())
}
