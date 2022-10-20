/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use fs_utils_rs::AntlirPath;
use pyo3::create_exception;
use pyo3::prelude::*;

create_exception!(artifacts_dir, SubvolNotFound, pyo3::exceptions::PyException);

#[pymodule]
pub fn find_built_subvol_rs(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SubvolNotFound", py.get_type::<SubvolNotFound>())?;

    #[pyfn(m)]
    fn find_built_subvol_internal(
        layer_output: AntlirPath,
        subvolumes_dir: Option<AntlirPath>,
        buck_root: AntlirPath,
    ) -> PyResult<AntlirPath> {
        let layer_output: PathBuf = layer_output.into();
        let subvolumes_dir: Option<PathBuf> = subvolumes_dir.map(|p| p.into());
        let buck_root: PathBuf = buck_root.into();

        let layer_output: AbsolutePathBuf = layer_output.try_into().map_err(|e| {
            SubvolNotFound::new_err(format!(
                "layer_output_internal AbsolutePathBuf conversion failed with {}",
                e,
            ))
        })?;

        let subvolumes_dir = match subvolumes_dir {
            Some(subvolumes_dir) => Some(subvolumes_dir.try_into().map_err(|e| {
                SubvolNotFound::new_err(format!(
                    "subvolumes_dir_internal AbsolutePathBuf conversion failed with {}",
                    e,
                ))
            })?),
            None => None,
        };

        let buck_root: AbsolutePathBuf = buck_root.try_into().map_err(|e| {
            SubvolNotFound::new_err(format!(
                "buck_root_internal AbsolutePathBuf conversion failed with {}",
                e,
            ))
        })?;

        match find_built_subvol_rust_lib::find_built_subvol(layer_output, subvolumes_dir, buck_root)
        {
            Ok(path) => Ok(path.into()),
            Err(e) => Err(SubvolNotFound::new_err(e.to_string())),
        }
    }

    Ok(())
}
