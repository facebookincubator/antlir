/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use artifacts_dir_rs::ensure_path_in_repo;
use fs_utils_rs::AntlirPath;
use pyo3::create_exception;
use pyo3::prelude::*;

create_exception!(artifacts_dir, SubvolNotFound, pyo3::exceptions::PyException);

#[pymodule]
pub fn find_built_subvol_rs(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SubvolNotFound", py.get_type::<SubvolNotFound>())?;

    #[pyfn(m)]
    fn find_built_subvol_internal(
        py: Python<'_>,
        layer_output: AntlirPath,
        subvolumes_dir: Option<AntlirPath>,
        path_in_repo: Option<AntlirPath>,
    ) -> PyResult<AntlirPath> {
        let layer_output: PathBuf = layer_output.into();
        let subvolumes_dir: Option<PathBuf> = subvolumes_dir.map(|p| p.into());
        let path_in_repo: Option<PathBuf> = path_in_repo.map(|p| p.into());

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

        let path_in_repo = {
            if subvolumes_dir.is_some() {
                None
            } else {
                let path_maybe_in_repo = ensure_path_in_repo(py, path_in_repo)?;
                match find_root::find_buck_cell_root(&path_maybe_in_repo) {
                    Ok(path) => Some(path),
                    Err(e) => return Err(SubvolNotFound::new_err(e.to_string())),
                }
            }
        };

        match find_built_subvol_rust_lib::find_built_subvol(
            layer_output,
            subvolumes_dir,
            path_in_repo,
        ) {
            Ok(path) => Ok(path.into()),
            Err(e) => Err(SubvolNotFound::new_err(e.to_string())),
        }
    }

    Ok(())
}
