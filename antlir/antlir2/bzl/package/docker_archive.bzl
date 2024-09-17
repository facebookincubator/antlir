# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "LayerInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":defs.bzl", "common_attrs", "default_attrs")
load(":macro.bzl", "package_macro")
load(":oci.bzl", "oci_attrs", "oci_rule")

def _impl(ctx: AnalysisContext) -> Promise:
    build_appliance = ctx.attrs.build_appliance or ctx.attrs.layer[LayerInfo].build_appliance

    def with_anon(oci) -> list[Provider]:
        out = ctx.actions.declare_output(ctx.label.name)

        oci = ensure_single_output(oci)
        spec = ctx.actions.write_json(
            "spec.json",
            {"docker_archive": {
                "build_appliance": build_appliance[BuildApplianceInfo].dir,
                "oci": oci,
            }},
            with_inputs = True,
        )
        ctx.actions.run(
            cmd_args(
                ctx.attrs._antlir2_packager[RunInfo],
                "--rootless",
                cmd_args(out.as_output(), format = "--out={}"),
                cmd_args(spec, format = "--spec={}"),
            ),
            category = "antlir2_package",
            identifier = "docker_archive",
        )
        return [
            DefaultInfo(out, sub_targets = {"oci": [DefaultInfo(oci)]}),
        ]

    all_attrs = {
        k: getattr(ctx.attrs, k)
        for k in list(oci_attrs) + list(layer_attrs) + list(common_attrs) + list(default_attrs) + ["_rootless"]
    }

    return ctx.actions.anon_target(
        oci_rule,
        {"name": str(ctx.attrs.layer.label.raw_target())} | all_attrs,
    ).promise.map(with_anon)

_docker_archive = rule(
    impl = _impl,
    attrs = oci_attrs | layer_attrs | default_attrs | common_attrs,
    cfg = package_cfg,
)

docker_archive = package_macro(_docker_archive)
