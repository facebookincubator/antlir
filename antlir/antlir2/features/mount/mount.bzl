# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")
load("//antlir/bzl:types.bzl", "types")

# IMO this is a misfeature, but it is used in many places throughout the legacy
# antlir1 world so we need to keep it around for a while
DefaultMountpointInfo = provider(fields = ["default_mountpoint"])

def layer_mount(
        *,
        source: str | Select,
        mountpoint: str | None = None) -> ParseTimeFeature:
    return ParseTimeFeature(
        feature_type = "mount",
        plugin = antlir2_dep("//antlir/antlir2/features/mount:mount"),
        deps = {
            "layer": source,
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
        source: str,
        is_directory: bool,
        mountpoint: str | None = None) -> ParseTimeFeature:
    mountpoint = mountpoint or source
    return ParseTimeFeature(
        feature_type = "mount",
        plugin = antlir2_dep("//antlir/antlir2/features/mount:mount"),
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
    mountpoint = str,
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

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.source_kind == "layer":
        mountpoint = ctx.attrs.mountpoint
        if not mountpoint:
            mountpoint = ctx.attrs.layer[DefaultMountpointInfo].default_mountpoint
        return [
            DefaultInfo(),
            FeatureAnalysis(
                feature_type = "mount",
                data = mount_record(
                    layer = layer_mount_record(
                        src = layer_dep_analyze(ctx.attrs.layer),
                        mountpoint = mountpoint,
                    ),
                    host = None,
                ),
                required_layers = [ctx.attrs.layer[LayerInfo]],
                plugin = ctx.attrs.plugin[FeaturePluginInfo],
            ),
        ]
    elif ctx.attrs.source_kind == "host":
        return [
            DefaultInfo(),
            FeatureAnalysis(
                feature_type = "mount",
                data = mount_record(
                    host = host_mount_record(
                        src = ctx.attrs.host_source,
                        mountpoint = ctx.attrs.mountpoint,
                        is_directory = ctx.attrs.is_directory,
                    ),
                    layer = None,
                ),
                plugin = ctx.attrs.plugin[FeaturePluginInfo],
            ),
        ]
    else:
        fail("invalid source_kind '{}'".format(ctx.attrs.source_kind))

mount_rule = rule(
    impl = _impl,
    attrs = {
        "host_source": attrs.option(attrs.string()),
        "is_directory": attrs.option(attrs.bool()),
        "layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "mountpoint": attrs.option(attrs.string()),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "source_kind": attrs.enum(["layer", "host"]),
        "_implicit_from_antlir1": attrs.bool(default = False),
    },
)
