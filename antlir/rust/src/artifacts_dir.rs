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

use crate::fs_utils::AntlirPath;

fn ensure_path_in_repo(py: Python<'_>, path_in_repo: Option<PathBuf>) -> PyResult<PathBuf> {
    match path_in_repo {
        Some(p) => Ok(p),
        None => match std::env::var_os("ANTLIR_PATH_IN_REPO") {
            Some(p) => Ok(p.into()),
            None => {
                let argv0: String = py.import("sys")?.getattr("argv")?.get_item(0)?.extract()?;
                Ok(argv0.into())
            }
        },
    }
}

#[pymodule]
pub(crate) fn artifacts_dir(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SigilNotFound", py.get_type::<SigilNotFound>())?;

    /// Find the path of the VCS repository root.  This could be the same thing
    /// as `find_buck_cell_root` but importantly, it might not be.  Buck has the
    /// concept of cells, of which many can be contained within a single VCS
    /// repository.  When you need to know the actual root of the VCS repo, use
    /// this method.
    #[pyfn(m)]
    fn find_repo_root(py: Python<'_>, path_in_repo: Option<AntlirPath>) -> PyResult<AntlirPath> {
        let path_in_repo = ensure_path_in_repo(py, path_in_repo.map(|p| p.into()))?;
        match find_root::find_repo_root(&path_in_repo) {
            Ok(path) => Ok(path.to_path_buf().into()),
            Err(e) => Err(SigilNotFound::new_err(e.to_string())),
        }
    }

    /// If the caller does not provide a path known to be in the repo, a reasonable
    /// default of sys.argv[0] will be used. This is reasonable as binaries/tests
    /// calling this library are also very likely to be in repo.

    /// This is intended to work:
    ///  - under Buck's internal macro interpreter, and
    ///  - using the system python from `facebookincubator/antlir`.

    /// This is functionally equivalent to `buck root`, but we opt to do it here as
    /// `buck root` takes >2s to execute (due to CLI startup time).
    #[pyfn(m)]
    fn find_buck_cell_root(
        py: Python<'_>,
        path_in_repo: Option<AntlirPath>,
    ) -> PyResult<AntlirPath> {
        let path_in_repo = ensure_path_in_repo(py, path_in_repo.map(|p| p.into()))?;
        match find_root::find_buck_cell_root(&path_in_repo) {
            Ok(path) => Ok(path.to_path_buf().into()),
            Err(e) => Err(SigilNotFound::new_err(e.to_string())),
        }
    }

    Ok(())
}
