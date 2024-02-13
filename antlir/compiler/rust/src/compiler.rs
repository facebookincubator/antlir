/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use buck_label::Label;
use clap::Parser;
use fs_utils_rs::AntlirPath;
use pyo3::prelude::*;
use targets_and_outputs_py::TargetsAndOutputs as TargetsAndOutputsPy;

/// This is normally invoked by the `image_layer` Buck macro converter.
///
/// This compiler builds a btrfs subvolume in
///   <--subvolumes-dir>/<--subvolume-rel-path>
///
/// To do so, it parses `--child-feature-json` and the `--child-dependencies`
/// that referred therein, creates `ImageItems`, sorts them in dependency order,
/// and invokes `.build()` to apply each item to actually construct the subvol.
#[derive(Parser)]
#[clap(no_binary_name(true))]
#[pyclass]
struct Args {
    #[clap(flatten)]
    debug: DebugArgs,
    #[clap(flatten)]
    working_volume: WorkingVolumeArgs,
    #[clap(flatten)]
    layer: LayerArgs,
    #[clap(flatten)]
    build_settings: BuildSettingsArgs,
}

#[derive(Parser)]
struct DebugArgs {
    /// Log more
    #[clap(long, env = "ANTLIR_DEBUG")]
    debug: bool,
    /// Profile this image build and write pstats files into the given
    /// directory.
    #[clap(long, overrides_with = "profile")]
    profile: Option<PathBuf>,
}

/// Args related to where the image build happens on host
#[derive(Parser)]
struct WorkingVolumeArgs {
    /// A directory on a btrfs volume to store the compiled subvolume
    /// representing the new layer
    #[clap(long)]
    subvolumes_dir: PathBuf,
    /// Path underneath --subvolumes-dir where we should create the subvolume.
    /// Note that all path components but the basename should already exist
    #[clap(long)]
    subvolume_rel_path: PathBuf,
}

/// Args related to the specific layer being built
#[derive(Parser)]
struct LayerArgs {
    /// The Buck target describing the layer being built
    #[clap(long)]
    child_layer_target: Label,
    /// The path of the JSON output of any `feature`s that are directly included
    /// by the layer being built
    #[clap(long)]
    child_feature_json: Vec<PathBuf>,
    /// The directory of the buck image output of the parent layer.  We will
    /// read the flavor from the parent layer to deduce the flavor of the child
    /// layer
    #[clap(long)]
    parent_layer: Option<PathBuf>,
    /// The serialized config for the flavor. This contains information about
    /// the build appliance and rpm installer
    #[clap(long, parse(try_from_str=serde_json::from_str))]
    flavor_config: Option<constants::flavor_config_t>,
    /// Path to a file containing TAB-separated ENVRAs, one per line. Also refer
    /// to `build_opts.bzl`
    #[clap(long)]
    version_set_override: Option<PathBuf>,
    /// Indicates whether the layer being compiled is a genrule layer.
    /// This is a temporary crutch to avoid running the compiler inside a BA
    /// container when building genrule layers. This should be removed in
    /// the future.
    #[clap(long)]
    internal_only_is_genrule_layer: bool,
    #[clap(long = "--targets-and-outputs")]
    targets_and_outputs_path: Option<PathBuf>,
    #[clap(skip)]
    targets_and_outputs_py: Option<TargetsAndOutputsPy>,
}

/// Args related to build settings that are not explicitly about the layer being
/// built, but instead configure the "environment"
#[derive(Parser)]
struct BuildSettingsArgs {
    /// Buck @mode/dev produces "in-place" build artifacts that are not truly
    /// standalone. It is important to be able to execute code from images built
    /// in this mode to support rapid development and debugging, even though it
    /// is not a "true" self-contained image. To allow execution of in-place
    /// binaries, antlir runtimes will automatically mount the repo into any
    /// `--artifacts-may-require-repo` image at runtime (e.g. when running image
    /// unit-tests, when using `=container` or `=systemd` targets, when using
    /// the image as a build appliance)
    #[clap(long)]
    artifacts_may_require_repo: bool,
    /// Target name that is allowed to contain host mounts used as
    /// build_sources. Can be specified more than once.
    #[clap(long)]
    allowed_host_mount_target: Vec<Label>,
    /// The path to the compiler binary being invoked currently.
    /// It is used to re-invoke the compiler inside the BA container as root.
    #[clap(long)]
    compiler_binary: PathBuf,
    /// Indicates whether the compiler binary is being run nested inside a BA
    /// container
    #[clap(long)]
    is_nested: bool,
}

#[pymethods]
impl Args {
    #[getter]
    fn debug(&self) -> bool {
        self.debug.debug
    }

    #[getter]
    fn profile_dir(&self) -> Option<AntlirPath> {
        self.debug
            .profile
            .as_deref()
            .map(|p| p.to_path_buf().into())
    }

    #[getter]
    fn subvolumes_dir(&self) -> AntlirPath {
        self.working_volume.subvolumes_dir.clone().into()
    }

    #[getter]
    fn subvolume_rel_path(&self) -> AntlirPath {
        self.working_volume.subvolume_rel_path.clone().into()
    }

    #[getter]
    fn child_layer_target(&self) -> buck_label_py::Label {
        buck_label_py::Label::new(self.layer.child_layer_target.to_owned())
    }

    #[getter]
    fn child_feature_json(&self) -> Vec<AntlirPath> {
        self.layer
            .child_feature_json
            .iter()
            .map(|p| p.to_path_buf().into())
            .collect()
    }

    #[getter]
    fn parent_layer(&self) -> Option<AntlirPath> {
        self.layer
            .parent_layer
            .as_deref()
            .map(|p| p.to_path_buf().into())
    }

    #[getter]
    fn flavor_config<'p>(&self) -> Option<&constants::flavor_config_t> {
        self.layer.flavor_config.as_ref()
    }

    #[getter]
    fn version_set_override(&self) -> Option<AntlirPath> {
        self.layer
            .version_set_override
            .as_deref()
            .map(|p| p.to_path_buf().into())
    }

    #[getter]
    fn internal_only_is_genrule_layer(&self) -> bool {
        self.layer.internal_only_is_genrule_layer
    }

    #[getter]
    fn targets_and_outputs(&self) -> Option<TargetsAndOutputsPy> {
        self.layer.targets_and_outputs_py.clone()
    }

    #[getter]
    fn artifacts_may_require_repo(&self) -> bool {
        self.build_settings.artifacts_may_require_repo
    }

    #[getter]
    fn allowed_host_mount_target(&self) -> Vec<buck_label_py::Label> {
        self.build_settings
            .allowed_host_mount_target
            .iter()
            .map(Label::to_owned)
            .map(buck_label_py::Label::new)
            .collect()
    }

    #[getter]
    fn compiler_binary(&self) -> AntlirPath {
        self.build_settings.compiler_binary.clone().into()
    }

    #[getter]
    fn is_nested(&self) -> bool {
        self.build_settings.is_nested
    }
}

py_utils::wrap_err!(
    AbsolutePathError,
    absolute_path::Error,
    pyo3::exceptions::PyException
);

py_utils::wrap_err!(FindRootError, anyhow::Error, pyo3::exceptions::PyException);

#[pymodule]
pub fn compiler(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Args>()?;

    #[pyfn(m)]
    fn parse_args(py: Python<'_>, argv: Vec<AntlirPath>) -> PyResult<Args> {
        let mut args =
            Args::parse_from(argv.into_iter().map(|p| p.as_path().as_os_str().to_owned()));

        // TODO: figure out some better way to cache computed properties if this grows
        args.layer.targets_and_outputs_py = match &args.layer.targets_and_outputs_path {
            Some(path) => Some(TargetsAndOutputsPy::from_argparse(
                py,
                path.clone().into(),
                None,
            )?),
            None => None,
        };

        Ok(args)
    }

    Ok(())
}
