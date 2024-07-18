/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use pyo3::create_exception;
use pyo3::prelude::*;

create_exception!(artifacts_dir, SigilNotFound, pyo3::exceptions::PyException);

use fs_utils_rs::AntlirPath;

fn ensure_path_in_repo(py: Python<'_>, path_in_repo: Option<PathBuf>) -> PyResult<PathBuf> {
    match path_in_repo {
        Some(p) => Ok(p),
        None => {
            let argv0: String = py.import("sys")?.getattr("argv")?.get_item(0)?.extract()?;
            let argv0 = PathBuf::from(argv0);
            Ok(argv0.canonicalize()?)
        }
    }
}

#[pymodule]
pub fn artifacts_dir(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SigilNotFound", py.get_type::<SigilNotFound>())?;

    /// find_repo_root($self, path_in_repo = None)
    /// --
    ///
    /// Find the path of the VCS repository root.
    #[pyfn(m)]
    fn find_repo_root(py: Python<'_>, path_in_repo: Option<AntlirPath>) -> PyResult<AntlirPath> {
        let path_in_repo = ensure_path_in_repo(py, path_in_repo.map(|p| p.into()))?;
        match find_root::find_repo_root(path_in_repo) {
            Ok(path) => Ok(path.into()),
            Err(e) => Err(SigilNotFound::new_err(e.to_string())),
        }
    }

    Ok(())
}
