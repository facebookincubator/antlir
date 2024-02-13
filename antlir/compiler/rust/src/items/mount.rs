/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
NB: Before this is able to be used in mount.py to replace the corresponding item classes
and mounts_from_meta, we need to resolve issues with pickling the Rust mount item
objects (which occurs in antlir/rpm/replay/rpm_replay.py).
*/

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs::File;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use find_built_subvol_rust_lib::find_built_subvol;
use fs_utils_rs::AntlirPath;
use pyo3::basic::CompareOp;
use pyo3::create_exception;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use targets_and_outputs_py::TargetsAndOutputs as TargetsAndOutputsPy;
use thiserror::Error;
use walkdir::WalkDir;

// Keep in sync w/ MOUNT_MARKER in mount_utils.py
const MOUNT_MARKER: &str = "MOUNT";

#[derive(Error, Debug)]
pub enum MountError {
    #[error("{0} was not a prefix of {1}")]
    UnrecognizedPrefix(PathBuf, PathBuf),
    #[error("Walking mount dir failed: {0}")]
    FailedToWalkDir(walkdir::Error),
    #[error("Filesystem error while building mount items for path {0}: {1}")]
    FilesystemError(PathBuf, std::io::Error),
    #[error("For file {0} JSON parse failed: {1}")]
    JSONParseError(PathBuf, serde_json::Error),
    #[error("Bad mount source for {0}: {1}")]
    BadMountSource(String, String),
    #[error("failed to absolutize path: {0}")]
    AbsolutePathError(absolute_path::Error),
    #[error("failed to find built subvol: {0}")]
    FindBuiltSubvolError(find_built_subvol_rust_lib::FindBuiltSubvolError),
}

impl std::convert::From<MountError> for PyErr {
    fn from(err: MountError) -> PyErr {
        PyRuntimeError::new_err(err.to_string())
    }
}

pub trait PyDataclass:
    Hash + Eq + PartialEq + Sized + Debug + serde::Serialize + serde::Deserialize<'static>
{
    fn __richcmp__(&self, other: Self, op: CompareOp) -> PyResult<bool> {
        match op {
            CompareOp::Eq => Ok(self == &other),
            CompareOp::Ne => Ok(self != &other),
            _ => Err(pyo3::exceptions::PyNotImplementedError::new_err(())),
        }
    }

    // https://github.com/PyO3/pyo3/issues/2122
    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("{:#?}", self))
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self))
    }

    fn dump_json(&self) -> PyResult<String> {
        serde_json::to_string(&self).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
}

// TODO(jtru): Remove the new(..) implementations when mounts_from_meta is dropped from
// mount.py (when back compat is no longer needed)
#[derive(
    serde::Serialize,
    serde::Deserialize,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Clone,
    Debug,
    Hash
)]
#[pyclass(dict, module = "antlir.compiler.rust.mount")]
pub struct BuildSource {
    #[pyo3(get)]
    pub r#type: String,
    #[pyo3(get)]
    pub source: String,
}

#[pymethods]
impl BuildSource {
    #[new]
    fn new(r#type: String, source: String) -> Self {
        BuildSource { r#type, source }
    }
    pub fn to_path(
        &self,
        target_to_path: TargetsAndOutputsPy,
        subvolumes_dir: AntlirPath,
    ) -> PyResult<PathBuf> {
        match self.r#type.as_str() {
            "layer" => {
                let subvolumes_dir: PathBuf = subvolumes_dir.into();
                let subvolumes_dir: AbsolutePathBuf = subvolumes_dir
                    .try_into()
                    .map_err(MountError::AbsolutePathError)?;
                match target_to_path.get(&self.source)? {
                    Some(out_path) => {
                        let out_path: AbsolutePathBuf = PathBuf::from(out_path.as_ref())
                            .try_into()
                            .map_err(MountError::AbsolutePathError)?;
                        Ok(
                            find_built_subvol(out_path, Some(subvolumes_dir), None, None)
                                .map_err(MountError::FindBuiltSubvolError)?
                                .into(),
                        )
                    }
                    None => Err(MountError::BadMountSource(
                        self.source.to_owned(),
                        format!("Target {} missing in target_to_path", self.source),
                    )),
                }
            }
            "host" => Ok(PathBuf::from(self.source.to_owned())),
            unknown => Err(MountError::BadMountSource(
                self.source.to_owned(),
                format!("Unrecognized type {}", unknown),
            )),
        }
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    pub fn __richcmp__(&self, other: Self, op: CompareOp) -> PyResult<bool> {
        PyDataclass::__richcmp__(self, other, op)
    }

    pub fn __hash__(&self) -> u64 {
        PyDataclass::__hash__(self)
    }

    pub fn __str__(&self) -> PyResult<String> {
        PyDataclass::__str__(self)
    }

    pub fn __repr__(&self) -> PyResult<String> {
        PyDataclass::__repr__(self)
    }

    pub fn dump_json(&self) -> PyResult<String> {
        PyDataclass::dump_json(self)
    }
}

impl PyDataclass for BuildSource {}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Debug,
    Clone,
    Hash
)]
#[pyclass(dict)]
pub struct RuntimeSource {
    #[pyo3(get)]
    pub r#type: String,
    // Note: these are specific to the FB runtime
    #[pyo3(get)]
    pub package: Option<String>,
    #[pyo3(get)]
    pub tag: Option<String>,
    #[pyo3(get)]
    pub uuid: Option<String>,
}

#[pymethods]
impl RuntimeSource {
    #[new]
    fn new(
        r#type: String,
        package: Option<String>,
        tag: Option<String>,
        uuid: Option<String>,
    ) -> Self {
        RuntimeSource {
            r#type,
            package,
            tag,
            uuid,
        }
    }

    pub fn __richcmp__(&self, other: Self, op: CompareOp) -> PyResult<bool> {
        PyDataclass::__richcmp__(self, other, op)
    }

    pub fn __hash__(&self) -> u64 {
        PyDataclass::__hash__(self)
    }

    pub fn __str__(&self) -> PyResult<String> {
        PyDataclass::__str__(self)
    }

    pub fn __repr__(&self) -> PyResult<String> {
        PyDataclass::__repr__(self)
    }

    pub fn dump_json(&self) -> PyResult<String> {
        PyDataclass::dump_json(self)
    }
}

impl PyDataclass for RuntimeSource {}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Debug,
    Clone,
    Hash
)]
#[pyclass(dict)]
pub struct LayerPublisher {
    #[pyo3(get)]
    pub package: String,
    // JSON contents of a shape target which can then be parsed
    #[pyo3(get)]
    pub shape_target_contents: String,
}

#[pymethods]
impl LayerPublisher {
    #[new]
    fn new(
        package: String,
        // JSON contents of a shape target which can then be parsed
        shape_target_contents: String,
    ) -> Self {
        LayerPublisher {
            package,
            shape_target_contents,
        }
    }

    pub fn __richcmp__(&self, other: Self, op: CompareOp) -> PyResult<bool> {
        PyDataclass::__richcmp__(self, other, op)
    }

    pub fn __hash__(&self) -> u64 {
        PyDataclass::__hash__(self)
    }

    pub fn __str__(&self) -> PyResult<String> {
        PyDataclass::__str__(self)
    }

    pub fn __repr__(&self) -> PyResult<String> {
        PyDataclass::__repr__(self)
    }

    pub fn dump_json(&self) -> PyResult<String> {
        PyDataclass::dump_json(self)
    }
}

impl PyDataclass for LayerPublisher {}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Debug,
    Hash,
    Clone
)]
#[pyclass(dict)]
pub struct Mount {
    #[pyo3(get)]
    pub mountpoint: PathBuf,
    #[pyo3(get)]
    pub build_source: BuildSource,
    #[pyo3(get)]
    pub is_directory: bool,
    #[pyo3(get)]
    pub runtime_source: Option<RuntimeSource>,
    #[pyo3(get)]
    pub layer_publisher: Option<LayerPublisher>,
}

#[pymethods]
impl Mount {
    #[new]
    fn new(
        mountpoint: PathBuf,
        build_source: BuildSource,
        is_directory: bool,
        runtime_source: Option<RuntimeSource>,
        layer_publisher: Option<LayerPublisher>,
    ) -> Self {
        Mount {
            mountpoint,
            build_source,
            is_directory,
            runtime_source,
            layer_publisher,
        }
    }

    pub fn __richcmp__(&self, other: Self, op: CompareOp) -> PyResult<bool> {
        PyDataclass::__richcmp__(self, other, op)
    }

    pub fn __hash__(&self) -> u64 {
        PyDataclass::__hash__(self)
    }

    pub fn __str__(&self) -> PyResult<String> {
        PyDataclass::__str__(self)
    }

    pub fn __repr__(&self) -> PyResult<String> {
        PyDataclass::__repr__(self)
    }

    pub fn dump_json(&self) -> PyResult<String> {
        PyDataclass::dump_json(self)
    }
}

impl PyDataclass for Mount {}

pub fn mounts_from_meta(volume_path: &Path) -> Result<Vec<Mount>, MountError> {
    let mut mounts: Vec<Mount> = Vec::new();
    let mounts_path = volume_path.join(".meta/private/mount");
    if !mounts_path
        .try_exists()
        .map_err(|e| MountError::FilesystemError(mounts_path.clone(), e))?
    {
        return Ok(mounts);
    }

    // Explicitly disable follow_links as we are not `chroot`ed and thus following links
    // could access paths outside the image
    for result in WalkDir::new(&mounts_path).follow_links(false) {
        let result = result.map_err(MountError::FailedToWalkDir)?;
        let abspath = result.path().to_path_buf();
        let relpath = abspath
            .strip_prefix(&mounts_path)
            .map_err(|_e| MountError::UnrecognizedPrefix(mounts_path.clone(), abspath.clone()))?;
        if relpath.file_name() != Some(OsStr::new(MOUNT_MARKER)) {
            continue;
        }
        let json_cfg_file = &abspath.join("mount_config.json");
        let mount_cfg_file = File::open(json_cfg_file)
            .map_err(|e| MountError::FilesystemError(json_cfg_file.clone(), e))?;
        let reader = BufReader::new(mount_cfg_file);
        mounts.push(
            serde_json::from_reader(reader)
                .map_err(|e| MountError::JSONParseError(abspath.clone(), e))?,
        );
    }
    Ok(mounts)
}

create_exception!(antlir, MountsFromMetaError, pyo3::exceptions::PyException);

#[pymodule]
pub fn mount(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<BuildSource>()?;
    m.add_class::<RuntimeSource>()?;
    m.add_class::<LayerPublisher>()?;
    m.add_class::<Mount>()?;

    m.add("MountsFromMetaError", py.get_type::<MountsFromMetaError>())?;

    #[pyfn(m)]
    fn mounts_from_meta_internal(_py: Python<'_>, volume_path: AntlirPath) -> PyResult<Vec<Mount>> {
        let volume_path: PathBuf = volume_path.into();
        mounts_from_meta(&volume_path).map_err(|e| MountsFromMetaError::new_err(e.to_string()))
    }

    Ok(())
}
