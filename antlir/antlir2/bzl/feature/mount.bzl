# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl:types.bzl", "types")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":ensure_dirs_exist.bzl", "ensure_subdirs_exist")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeDependency", "ParseTimeFeature")
load(":install.bzl", "install")

def layer_mount(
        *,
        source: [str.type, "selector"],
        mountpoint: [str.type, None] = None,
        _implicit_from_antlir1: bool.type = False) -> [ParseTimeFeature.type]:
    features = [
        ParseTimeFeature(
            feature_type = "mount",
            deps = {
                "source": ParseTimeDependency(dep = source, providers = [LayerInfo]),
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

    # TODO(T153572212): antlir2 requires the image author to pre-create the mountpoint
    if _implicit_from_antlir1 and mountpoint:
        features.extend(
            ensure_subdirs_exist(
                into_dir = paths.dirname(mountpoint),
                subdirs_to_create = paths.basename(mountpoint),
            ),
        )
    return features

def host_mount(
        *,
        source: str.type,
        is_directory: bool.type,
        mountpoint: [str.type, None] = None,
        _implicit_from_antlir1: bool.type = False) -> [ParseTimeFeature.type]:
    mountpoint = mountpoint or source
    features = [ParseTimeFeature(
        feature_type = "mount",
        kwargs = {
            "host_source": source,
            "is_directory": is_directory,
            "mountpoint": mountpoint,
            "source_kind": "host",
            "_implicit_from_antlir1": False,
        },
        deps = {},
    )]

    # TODO(T153572212): antlir2 requires the image author to pre-create the mountpoint
    if _implicit_from_antlir1 and mountpoint:
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
    mountpoint = [str.type, None],
    src = layer_dep.type,
)

host_mount_record = record(
    mountpoint = str.type,
    src = str.type,
    is_directory = bool.type,
)

mount_record = record(
    layer = [layer_mount_record.type, None],
    host = [host_mount_record.type, None],
)

def mount_analyze(
        mountpoint: [str.type, None],
        source_kind: _source_kind.type,
        is_directory: [bool.type, None],
        host_source: [str.type, None],
        _implicit_from_antlir1: bool.type,
        deps: {str.type: "dependency"} = {}) -> FeatureAnalysis.type:
    if source_kind == "layer":
        source = deps.pop("source")
        if not mountpoint:
            mountpoint = source[LayerInfo].default_mountpoint
        return FeatureAnalysis(
            feature_type = "mount",
            data = mount_record(
                layer = layer_mount_record(
                    src = layer_dep_analyze(source),
                    mountpoint = mountpoint,
                ),
                host = None,
            ),
            required_layers = [source[LayerInfo]],
        )
    elif source_kind == "host":
        return FeatureAnalysis(
            feature_type = "mount",
            data = mount_record(
                host = host_mount_record(
                    src = host_source,
                    mountpoint = mountpoint,
                    is_directory = is_directory,
                ),
                layer = None,
            ),
        )
    else:
        fail("invalid source_kind '{}'".format(source_kind))
