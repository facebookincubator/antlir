# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl:types.bzl", "types")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":ensure_dirs_exist.bzl", "ensure_dir_exists_analyze", "ensure_subdirs_exist")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeDependency", "ParseTimeFeature")
load(":install.bzl", "install")

# IMO this is a misfeature, but it is used in many places throughout the legacy
# antlir1 world so we need to keep it around for a while
DefaultMountpointInfo = provider(fields = ["default_mountpoint"])

def layer_mount(
        *,
        source: str | Select,
        mountpoint: str | None = None,
        mkdir: bool = False,
        _implicit_from_antlir1: bool = False) -> list[ParseTimeFeature]:
    features = [
        ParseTimeFeature(
            feature_type = "mount",
            plugin = antlir2_dep("features:mount"),
            deps = {
                "source": ParseTimeDependency(dep = source, providers = [LayerInfo]),
            },
            exec_deps = {
                "ensure_dir_exists_plugin": ParseTimeDependency(
                    dep = antlir2_dep("features:ensure_dir_exists"),
                    providers = [FeaturePluginInfo],
                ),
            },
            kwargs = {
                "host_source": None,
                "is_directory": None,
                "mountpoint": mountpoint,
                "source_kind": "layer",
                "_implicit_from_antlir1": _implicit_from_antlir1,
            },
        ),
    ]

    if not _implicit_from_antlir1 and not mountpoint:
        fail("antlir2 layer_mount requires mountpoint to be set")

    if mkdir:
        features.extend(ensure_subdirs_exist(
            into_dir = paths.dirname(mountpoint),
            subdirs_to_create = paths.basename(mountpoint),
        ))
    return features

def host_mount(
        *,
        source: str,
        is_directory: bool,
        mountpoint: str | None = None,
        create_mountpoint: bool = False,
        _implicit_from_antlir1: bool = False) -> list[ParseTimeFeature]:
    mountpoint = mountpoint or source
    features = [ParseTimeFeature(
        feature_type = "mount",
        plugin = antlir2_dep("features:mount"),
        kwargs = {
            "host_source": source,
            "is_directory": is_directory,
            "mountpoint": mountpoint,
            "source_kind": "host",
            "_implicit_from_antlir1": False,
        },
        deps = {},
        exec_deps = {
            "ensure_dir_exists_plugin": ParseTimeDependency(
                dep = antlir2_dep("features:ensure_dir_exists"),
                providers = [FeaturePluginInfo],
            ),
        },
    )]

    if create_mountpoint or _implicit_from_antlir1:
        if is_directory:
            features.extend(
                ensure_subdirs_exist(
                    into_dir = paths.dirname(mountpoint),
                    subdirs_to_create = paths.basename(mountpoint),
                ),
            )
        else:
            features.append(
                install(
                    src = antlir_dep(":empty"),
                    dst = mountpoint,
                ),
            )
    return features

host_file_mount = partial(host_mount, is_directory = False)
host_dir_mount = partial(host_mount, is_directory = True)

_source_kind = enum("layer", "host")
types.lint_noop(_source_kind)

layer_mount_record = record(
    # TODO: this is only nullable because implicit conversions from antlir1
    # don't correctly set this in many cases
    mountpoint = [str, None],
    src = layer_dep,
)

host_mount_record = record(
    mountpoint = str,
    src = str,
    is_directory = bool,
)

mount_record = record(
    layer = [layer_mount_record, None],
    host = [host_mount_record, None],
)

def mount_analyze(
        mountpoint: str | None,
        source_kind: str,
        is_directory: bool | None,
        host_source: str | None,
        _implicit_from_antlir1: bool,
        plugin: FeaturePluginInfo,
        deps: dict[str, Dependency] = {},
        exec_deps: dict[str, Dependency] = {}) -> list[FeatureAnalysis]:
    features = []
    if source_kind == "layer":
        source = deps.pop("source")
        if not mountpoint:
            mountpoint = source[DefaultMountpointInfo].default_mountpoint
        features.append(FeatureAnalysis(
            feature_type = "mount",
            data = mount_record(
                layer = layer_mount_record(
                    src = layer_dep_analyze(source),
                    mountpoint = mountpoint,
                ),
                host = None,
            ),
            required_layers = [source[LayerInfo]],
            plugin = plugin,
        ))
    elif source_kind == "host":
        features.append(FeatureAnalysis(
            feature_type = "mount",
            data = mount_record(
                host = host_mount_record(
                    src = host_source,
                    mountpoint = mountpoint,
                    is_directory = is_directory,
                ),
                layer = None,
            ),
            plugin = plugin,
        ))
    else:
        fail("invalid source_kind '{}'".format(source_kind))

    # TODO(T153572212): antlir2 requires the image author to pre-create the mountpoint
    if _implicit_from_antlir1:
        features.append(ensure_dir_exists_analyze(
            dir = mountpoint,
            mode = 0o755,
            user = "root",
            group = "root",
            plugin = exec_deps["ensure_dir_exists_plugin"][FeaturePluginInfo],
        ))
    return features
