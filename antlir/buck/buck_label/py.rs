/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use pyo3::class::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::PyString;

#[pyclass(module = "antlir.buck.buck_label.buck_label_py")]
#[derive(Clone)]
pub struct Label(buck_label::Label);

py_utils::wrap_err!(Error, buck_label::Error, pyo3::exceptions::PyValueError);

impl Label {
    pub fn new(label: buck_label::Label) -> Self {
        Self(label)
    }
}

#[pymethods]
impl Label {
    /// Empty constructor for pickle. Python user code should never call this.
    /// It's basically useless since it takes no parameters.
    #[new]
    fn __new__(s: &str) -> Result<Self, Error> {
        buck_label::Label::new(s)
            .map(|label| Self::new(label.to_owned()))
            .map_err(|e| e.into())
    }

    fn __richcmp__(&self, other: &PyAny, op: CompareOp) -> PyResult<bool> {
        let other = match other.extract::<Label>() {
            Ok(l) => l.0,
            Err(_) => {
                let other: &str = other.extract()?;
                buck_label::Label::new(other).map_err(Error::from)?
            }
        };
        match op {
            CompareOp::Lt => Ok(self.0 < other),
            CompareOp::Le => Ok(self.0 <= other),
            CompareOp::Eq => Ok(self.0 == other),
            CompareOp::Ne => Ok(self.0 != other),
            CompareOp::Gt => Ok(self.0 > other),
            CompareOp::Ge => Ok(self.0 >= other),
        }
    }

    fn __str__(&self) -> String {
        self.0.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Label('{}')", self.0)
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        let s = PyString::new(py, &self.0.to_string());
        s.hash()
    }

    #[getter]
    fn unconfigured(&self) -> Self {
        Self::new(self.0.as_unconfigured())
    }

    #[getter]
    fn cell(&self) -> &str {
        self.0.cell()
    }

    #[getter]
    fn package(&self) -> &str {
        self.0.package()
    }

    #[getter]
    fn name(&self) -> &str {
        self.0.name()
    }

    #[getter]
    fn config(&self) -> Option<&str> {
        self.0.config()
    }
}

#[pymodule]
pub fn buck_label_py(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Label>()?;

    Ok(())
}
