# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginPluginKind")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":attrs.bzl", "common_attrs", "default_attrs_base")
load(":cfg.bzl", "package_cfg")
load(":macro.bzl", "package_macro")
load(":stamp_buildinfo.bzl", "stamp_buildinfo_rule")

def _unprivileged_dir_impl_with_layer(
        layer: [Dependency, ProviderCollection],
        *,
        ctx: AnalysisContext) -> list[Provider]:
    output_name = ctx.attrs.out or ctx.label.name
    package = ctx.actions.declare_output(output_name, dir = True, has_content_based_path = False)

    encoded_path_mapping = ctx.actions.declare_output("encoded_path_mapping.json", has_content_based_path = False)
    if not ctx.attrs.base64_encode_filenames:
        ctx.actions.write_json(encoded_path_mapping.as_output(), {})
    spec = ctx.actions.write_json(
        "spec.json",
        {"unprivileged_dir": {
            "base64_encoded_filenames": encoded_path_mapping.as_output(),
        } if ctx.attrs.base64_encode_filenames else {}},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            cmd_args("sudo", "--preserve-env=TMPDIR") if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(layer[LayerInfo].contents.subvol_symlink, format = "--layer={}"),
            "--dir",
            cmd_args(package.as_output(), format = "--out={}"),
            "--rootless" if ctx.attrs._rootless else cmd_args(),
        ),
        local_only = True,
        category = "antlir2_package",
        identifier = "unprivileged_dir",
    )

    return [DefaultInfo(package, sub_targets = {
        "base64_encoded_path_mapping": [DefaultInfo(encoded_path_mapping)],
    })]

def _unprivileged_dir_impl(ctx: AnalysisContext):
    if ctx.attrs.dot_meta:
        return ctx.actions.anon_target(stamp_buildinfo_rule, {
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
            "_analyze_feature": ctx.attrs._analyze_feature,
            "_antlir2": ctx.attrs._antlir2,
            "_dot_meta_feature": ctx.attrs._dot_meta_feature,
            "_plugins": ctx.attrs._plugins + (ctx.plugins[FeaturePluginPluginKind] if FeaturePluginPluginKind in ctx.plugins else []),
            "_run_container": ctx.attrs._run_container,
        } | {
            k: getattr(ctx.attrs, k)
            for k in attrs_selected_by_cfg
        }).promise.map(partial(
            _unprivileged_dir_impl_with_layer,
            ctx = ctx,
        ))
    else:
        return _unprivileged_dir_impl_with_layer(
            layer = ctx.attrs.layer,
            ctx = ctx,
        )

_unprivileged_dir_attrs = {
    "base64_encode_filenames": attrs.bool(
        default = False,
        doc = "Encode filenames that contain invalid characters (/ or \\) in base64 so they are legal in buck-out",
    ),
    "dot_meta": attrs.bool(default = True),
}

_unprivileged_dir = rule(
    impl = _unprivileged_dir_impl,
    cfg = package_cfg,
    uses_plugins = [FeaturePluginPluginKind],
    attrs = default_attrs_base | common_attrs | _unprivileged_dir_attrs | attrs_selected_by_cfg,
)

unprivileged_dir_anon = anon_rule(
    impl = lambda ctx: _unprivileged_dir_impl_with_layer(ctx.attrs.layer, ctx = ctx),
    artifact_promise_mappings = {
        "base64_encoded_path_mapping": lambda x: ensure_single_output(x[DefaultInfo].sub_targets["base64_encoded_path_mapping"]),
        "package": lambda x: ensure_single_output(x),
    },
    attrs = default_attrs_base | common_attrs | _unprivileged_dir_attrs | attrs_selected_by_cfg,
)

unprivileged_dir = package_macro(_unprivileged_dir)
