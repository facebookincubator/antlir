# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def dot_meta(
        *,
        revision: [str, None] = None,
        package_name: [str, None] = None,
        package_version: [str, None] = None):
    """
    Stamp build info into /.meta in the built layer
    """
    revision = revision or native.read_config("build_info", "revision")
    package_name = package_name or native.read_config("build_info", "package_name")
    package_version = package_version or native.read_config("build_info", "package_version")
    if int(bool(package_name)) ^ int(bool(package_version)):
        warning("Only one of {package_name, package_version} was set; package info will not be materialized into .meta")

    package = None
    if package_name and package_version:
        package = package_name + ":" + package_version

    build_info = {
        "package": package,
        "revision": revision,
    }
    return ParseTimeFeature(
        feature_type = "dot_meta",
        plugin = "antlir//antlir/antlir2/features/dot_meta:dot_meta",
        kwargs = {
            "build_info": build_info,
        },
    )

build_info_record = record(
    revision = str | None,
    package = str | None,
)

dot_meta_record = record(
    build_info = build_info_record | None,
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "dot_meta",
            data = dot_meta_record(
                build_info = build_info_record(**ctx.attrs.build_info) if ctx.attrs.build_info else None,
            ),
            build_phase = BuildPhase("buildinfo_stamp"),
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

dot_meta_rule = rule(
    impl = _impl,
    attrs = {
        "build_info": attrs.dict(attrs.string(), attrs.option(attrs.string())),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
    },
)
