# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/bzl:types.bzl", "types")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeDependency", "ParseTimeFeature")

types.lint_noop()

def layer_mount(
        *,
        source: types.or_selector(str.type),
        mountpoint: [str.type, None] = None) -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "mount",
        deps = {
            "source": ParseTimeDependency(dep = source, providers = [LayerInfo]),
        },
        kwargs = {
            "host_source": None,
            "is_directory": None,
            "mountpoint": mountpoint,
            "source_kind": "layer",
        },
    )

def host_mount(
        *,
        source: str.type,
        is_directory: bool.type,
        mountpoint: [str.type, None] = None) -> ParseTimeFeature.type:
    mountpoint = mountpoint or source
    return ParseTimeFeature(
        feature_type = "mount",
        kwargs = {
            "host_source": source,
            "is_directory": is_directory,
            "mountpoint": mountpoint,
            "source_kind": "host",
        },
        deps = {},
    )

host_file_mount = partial(host_mount, is_directory = False)
host_dir_mount = partial(host_mount, is_directory = True)

_source_kind = enum("layer", "host")
types.lint_noop(_source_kind)

layer_mount_record = record(
    mountpoint = str.type,
    src = layer_dep.type,
)

host_mount_record = record(
    mountpoint = str.type,
    src = layer_dep.type,
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
        deps: {str.type: "dependency"}) -> FeatureAnalysis.type:
    if source_kind == "layer":
        source = deps.pop("source")
        if not mountpoint:
            default_mountpoint = source[LayerInfo].default_mountpoint
            if not default_mountpoint:
                fail("mountpoint is required if source does not have a default mountpoint")
            mountpoint = default_mountpoint
        return FeatureAnalysis(
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
