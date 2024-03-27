/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyString;

/// Wrapper for Antlir Paths (antlir.fs_utils.Path)
/// In Rust, this is a PathBuf, and in Python it is an antlir.fs_utils.Path
/// This intentionally implements very few useful traits directly, aside from
/// converting to a Path(Buf), because it should almost never be passed around
/// internally, instead it should only be used as function parameters or return
/// values.
#[repr(transparent)]
pub struct AntlirPath(PathBuf);

impl AntlirPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for AntlirPath {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

impl From<AntlirPath> for PathBuf {
    fn from(p: AntlirPath) -> PathBuf {
        p.0
    }
}

impl AsRef<Path> for AntlirPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl IntoPy<PyObject> for AntlirPath {
    fn into_py(self, py: Python) -> PyObject {
        // This is breaking our unwritten golden rule of never go
        // Python->Rust->Python, but it's a necessary indirection evil
        // until/unless we replace antlir.fs_utils.Path with a Rust
        // implementation
        let fs_utils =
            PyModule::import(py, "antlir.fs_utils").expect("antlir.fs_utils must be available");
        let bytes = PyBytes::new(py, self.0.as_os_str().as_bytes());
        // finally, create a fs_utils.Path from the bytes object
        fs_utils
            .getattr("Path")
            .expect("antlir.fs_utils always has Path")
            .call1((bytes,))
            .expect("antlir.fs_utils.Path failed to take a byte string")
            .into()
    }
}

impl<'source> FromPyObject<'source> for AntlirPath {
    fn extract(p: &'source PyAny) -> PyResult<Self> {
        // first attempt to get a raw bytes string, which most paths should
        // already be
        if let Ok(bytes) = p.downcast::<PyBytes>() {
            Ok(PathBuf::from(OsStr::from_bytes(bytes.as_bytes())).into())
        } else {
            // if it's not already `bytes`, then hopefully it's a `str`,
            // otherwise we are out of ideas and can't convert it
            match p.downcast::<PyString>() {
                Ok(str) => Ok(Self(str.to_str()?.into())),
                Err(_) => Err(PyTypeError::new_err(format!(
                    "{} is neither bytes nor str",
                    p.repr()?
                ))),
            }
        }
    }
}

#[pymodule]
pub fn fs_utils_rs(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    /// Largely just useful for tests from Python, this will take the given
    /// input and attempt to round-trip it through [Path] and back into an
    /// `antlir.fs_utils.Path`
    #[pyfn(m)]
    #[pyo3(name = "Path")]
    fn path(p: &PyAny) -> PyResult<AntlirPath> {
        p.extract()
    }

    Ok(())
}
