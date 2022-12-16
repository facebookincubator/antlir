/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::io::Cursor;
use std::ops::Deref;

use absolute_path::AbsolutePath;
use absolute_path::AbsolutePathBuf;
use anyhow::Context;
use artifacts_dir_rs::ensure_path_in_repo;
use buck_label::Label;
use fs_utils_rs::AntlirPath;
use pyo3::exceptions::PyKeyError;
use pyo3::exceptions::PyOSError;
use pyo3::exceptions::PyTypeError;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;
use serde::Deserialize;
use serde::Serialize;

/// Python-specific implementation of TargetsAndOutputs that converts all paths
/// to absolute paths on access.
#[pyclass(
    module = "antlir.buck.targets_and_outputs.targets_and_outputs_py",
    mapping
)]
#[derive(Clone, Deserialize, Serialize)]
pub struct TargetsAndOutputs {
    buck_cell_root: AbsolutePathBuf,
    #[serde(borrow)]
    inner: targets_and_outputs::TargetsAndOutputs<'static>,
}

py_utils::wrap_err!(Error, anyhow::Error, pyo3::exceptions::PyValueError);

impl From<buck_label::Error> for Error {
    fn from(e: buck_label::Error) -> Self {
        Self(e.into())
    }
}

impl From<absolute_path::Error> for Error {
    fn from(e: absolute_path::Error) -> Self {
        Self(e.into())
    }
}

impl From<pyo3::PyErr> for Error {
    fn from(e: pyo3::PyErr) -> Self {
        Self(e.into())
    }
}

impl TargetsAndOutputs {
    /// Legacy python code expects absolute paths for all targets and outputs
    /// usage, so convert it from the rust implementation which uses relpaths
    /// whenever possible (to play nicer with buck2 + caching)
    pub fn from_rust(
        tao: targets_and_outputs::TargetsAndOutputs<'static>,
        path_in_repo: &AbsolutePath,
    ) -> Result<Self, Error> {
        let buck_cell_root = find_root::find_buck_cell_root(path_in_repo)
            .context("while looking up buck cell root")?;
        Ok(Self {
            buck_cell_root,
            inner: tao,
        })
    }
}

#[pymethods]
impl TargetsAndOutputs {
    /// Empty constructor for pickle. Python user code should never call this.
    /// It's basically useless since it takes no parameters.
    #[new]
    fn __new__(json_str: Option<&str>) -> PyResult<Self> {
        if let Some(json_str) = json_str {
            let mut deser = serde_json::Deserializer::from_reader(Cursor::new(json_str));
            let s = Self::deserialize(&mut deser)
                .map_err(|e| PyValueError::new_err(format!("pickled data is invalid: {e}")))?;
            Ok(s)
        } else {
            Err(PyTypeError::new_err(
                "TargetsAndOutputs cannot be directly constructed",
            ))
        }
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __contains__(&self, key: &str) -> Result<bool, Error> {
        Ok(self.get(key)?.is_some())
    }

    fn __getitem__(&self, key: &str) -> PyResult<Option<AntlirPath>> {
        match self.get(key) {
            Ok(Some(v)) => Ok(Some(v)),
            Ok(None) => Err(PyKeyError::new_err(format!("'{key}'"))),
            Err(e) => Err(e.into()),
        }
    }

    /// Get an absolute path to the output of a buck rule.
    fn get(&self, key: &str) -> Result<Option<AntlirPath>, Error> {
        let label = Label::new(key)?;
        Ok(self
            .inner
            .path(&label)
            .map(|relpath| self.buck_cell_root.join(relpath).into()))
    }

    #[staticmethod]
    pub fn from_argparse(
        py: Python<'_>,
        path: AntlirPath,
        path_in_repo: Option<AntlirPath>,
    ) -> PyResult<Self> {
        Self::from_file(py, path, path_in_repo)
            .map_err(|e| PyValueError::new_err(format!("{:?}", e)))
    }

    #[staticmethod]
    fn from_file(
        py: Python<'_>,
        path: AntlirPath,
        path_in_repo: Option<AntlirPath>,
    ) -> Result<Self, Error> {
        let json_str = std::fs::read_to_string(path).map_err(PyOSError::new_err)?;
        Self::from_json_str(py, &json_str, path_in_repo)
    }

    #[staticmethod]
    fn from_json_str(
        py: Python<'_>,
        json_str: &str,
        path_in_repo: Option<AntlirPath>,
    ) -> Result<Self, Error> {
        let path_in_repo = ensure_path_in_repo(py, path_in_repo.map(|p| p.as_ref().to_path_buf()))?;
        let mut deser = serde_json::Deserializer::from_reader(Cursor::new(json_str));
        let inner = targets_and_outputs::TargetsAndOutputs::deserialize(&mut deser)
            .context("while deserializing")?;
        Self::from_rust(inner, &path_in_repo)
    }

    fn dict(&self) -> HashMap<String, AntlirPath> {
        self.inner
            .iter()
            .map(|(label, relpath)| (label.to_string(), self.buck_cell_root.join(relpath).into()))
            .collect()
    }

    // Pickling, fun!
    pub fn __reduce__<'py>(
        slf: &'py PyCell<Self>,
        py: Python<'py>,
    ) -> PyResult<(PyObject, &'py PyTuple)> {
        let cls = slf.to_object(py).getattr(py, "__class__")?;
        Ok((
            cls,
            PyTuple::new(
                py,
                [serde_json::to_string(slf.borrow().deref()).expect("infallible")],
            ),
        ))
    }
}

#[pymodule]
pub fn targets_and_outputs_py(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<TargetsAndOutputs>()?;

    Ok(())
}
