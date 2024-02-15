/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use pyo3::prelude::*;

/// Register a Rust module as a Python submodule. The Rust module must provide a
/// crate-public function with the same name as the module itself, that uses the
/// #[pymodule] macro as described in the PyO3 docs
/// (https://pyo3.rs/v0.16.4/module.html)
macro_rules! submodule {
    ($module:ident, $full_module_name:literal, $py:ident, $parent_module:ident) => {{
        let child_module = PyModule::new($py, stringify!($module))?;
        $module::$module($py, child_module)?;
        $parent_module.add_submodule(child_module)?;

        // Small hack to enable intuitive Python import resolution. All
        // submodules technically come from loading `antlir.rust`, but we want
        // them to be able to be imported based on where they are defined.
        // Inject all submodules into the `sys.modules` dictionary with the full
        // import name so that import resolution works like a user would expect.
        // Small note: there is no extra initialization cost to doing this like
        // there might be in pure-Python module imports, since all the
        // initialization is done as part of importing the top level
        // `antlir.rust` module in the first place.
        $py.import("sys")?
            .getattr("modules")?
            .set_item($full_module_name, child_module)?;
        Ok::<_, pyo3::PyErr>(())
    }};
}

mod register_modules;

#[pymodule]
#[pyo3(name = "native_antlir_impl")]
fn native(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    register_modules::register_modules(py, m)?;
    Ok(())
}
