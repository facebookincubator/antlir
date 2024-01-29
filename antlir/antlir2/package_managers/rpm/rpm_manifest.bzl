# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _rpm_manifest_impl(ctx: AnalysisContext) -> list[Provider]:
    manifest = ctx.actions.declare_output("manifest.json")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._rpm_manifest[RunInfo],
            cmd_args(ctx.attrs.layer[LayerInfo].facts_db, format = "--facts-db={}"),
            cmd_args(manifest.as_output(), format = "--out={}"),
        ),
        category = "rpm_manifest",
    )
    return [DefaultInfo(manifest)]

_rpm_manifest = rule(
    impl = _rpm_manifest_impl,
    attrs = {
        "layer": attrs.dep(providers = [LayerInfo]),
        "_rpm_manifest": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/package_managers/rpm:rpm-manifest")),
    },
)

rpm_manifest = rule_with_default_target_platform(_rpm_manifest)
