/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use pyo3::create_exception;
use pyo3::exceptions::PyFileNotFoundError;
use pyo3::prelude::*;

create_exception!(artifacts_dir, SigilNotFound, pyo3::exceptions::PyException);

use fs_utils_rs::AntlirPath;

fn ensure_path_in_repo(py: Python<'_>, path_in_repo: Option<PathBuf>) -> PyResult<AbsolutePathBuf> {
    let maybe_relpath = match path_in_repo {
        Some(p) => p,
        None => match std::env::var_os("ANTLIR_PATH_IN_REPO") {
            Some(p) => p.into(),
            None => {
                let argv0: String = py.import("sys")?.getattr("argv")?.get_item(0)?.extract()?;
                argv0.into()
            }
        },
    };
    AbsolutePathBuf::absolutize(&maybe_relpath).map_err(|e| match e {
        absolute_path::Error::Canonicalize(p, e) => match e.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(p.display().to_string()),
            _ => PyErr::from(e),
        },
        _ => SigilNotFound::new_err(format!(
            "'{}' could not be made absolute",
            maybe_relpath.display()
        )),
    })
}

#[pymodule]
pub fn artifacts_dir_rs(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SigilNotFound", py.get_type::<SigilNotFound>())?;

    /// find_repo_root($self, path_in_repo = None)
    /// --
    ///
    /// Find the path of the VCS repository root.  This could be the same thing
    /// as `find_buck_cell_root` but importantly, it might not be.  Buck has the
    /// concept of cells, of which many can be contained within a single VCS
    /// repository.  When you need to know the actual root of the VCS repo, use
    /// this method.
    #[pyfn(m)]
    fn find_repo_root(py: Python<'_>, path_in_repo: Option<AntlirPath>) -> PyResult<AntlirPath> {
        let path_in_repo = ensure_path_in_repo(py, path_in_repo.map(|p| p.into()))?;
        match find_root::find_repo_root(&path_in_repo) {
            Ok(path) => Ok(path.into_path_buf().into()),
            Err(e) => Err(SigilNotFound::new_err(e.to_string())),
        }
    }

    /// find_buck_cell_root($self, path_in_repo = None)
    /// --
    ///
    /// Find the root of the buck cell we're currently in.
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
            Ok(path) => Ok(path.into_path_buf().into()),
            Err(e) => Err(SigilNotFound::new_err(e.to_string())),
        }
    }

    Ok(())
}
