# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:layer.bzl", "layer_rule")
load("//antlir/antlir2/bzl/image:mounts.bzl", "all_mounts", "container_mount_args")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginPluginKind")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    out = None
    out_is_dir = False
    default_info = None

    if ctx.attrs.out and ctx.attrs.outs:
        fail("out and outs cannot be specified together")
    elif ctx.attrs.out:
        out_name = (None if ctx.attrs.out == "." else ctx.attrs.out.removesuffix("/")) or "out"
        if ctx.attrs.out == "." or ctx.attrs.out.endswith("/"):
            out_is_dir = True
            out = ctx.actions.declare_output(out_name, dir = True)
        else:
            out = ctx.actions.declare_output(out_name)
        default_info = DefaultInfo(out)
        if ctx.attrs.default_out:
            fail("default_out cannot be combined with out")
    elif ctx.attrs.outs:
        out_is_dir = True
        out = ctx.actions.declare_output("outs", dir = True)
        default_out = out
        if ctx.attrs.default_out:
            default_out = out.project(ctx.attrs.default_out)
        default_info = DefaultInfo(default_out, sub_targets = {
            name: [DefaultInfo(out.project(path))]
            for name, path in ctx.attrs.outs.items()
        })
    else:
        fail("out or outs is required")

    def _with_anon_layer(layer) -> list[Provider]:
        ctx.actions.run(
            cmd_args(
                "sudo" if not ctx.attrs._rootless else cmd_args(),
                ctx.attrs._genrule_in_image[RunInfo],
                "--rootless" if ctx.attrs._rootless else cmd_args(),
                cmd_args(layer[LayerInfo].contents.subvol_symlink, format = "--layer={}") if ctx.attrs._working_format == "btrfs" else cmd_args(),
                cmd_args(ctx.attrs._working_format, format = "--working-format={}"),
                cmd_args(out.as_output(), format = "--out={}"),
                "--dir" if out_is_dir else cmd_args(),
                cmd_args([
                    container_mount_args(mount)
                    for mount in all_mounts(
                        features = layer[LayerInfo].features,
                        parent_layer = layer[LayerInfo].parent[LayerInfo] if layer[LayerInfo].parent else None,
                    )
                ]),
                "--",
                ctx.attrs.bash,
            ),
            local_only = (
                # btrfs subvolumes can only exist locally
                ctx.attrs._working_format == "btrfs" or
                # no sudo access on remote execution
                not ctx.attrs._rootless or
                # no aarch64 emulation on remote execution
                ctx.attrs._target_arch == "aarch64"
            ),
            category = "antlir2_genrule",
        )
        return [
            default_info,
        ]

    if int(bool(ctx.attrs.layer)) + int(bool(ctx.attrs.exec_layer)) != 1:
        fail("exactly one of layer or exec_layer must be specified")

    if ctx.attrs.layer:
        return ctx.actions.anon_target(layer_rule, {
            "antlir2": ctx.attrs._layer_antlir2,
            "build_appliance": ctx.attrs.build_appliance,
            "flavor": ctx.attrs.flavor,
            "name": "genrule_layer//{}:{}".format(ctx.label.package, ctx.label.name),
            "parent_layer": ctx.attrs.layer,
            "rootless": ctx.attrs._rootless,
            "_analyze_feature": ctx.attrs._layer_analyze_feature,
            "_feature_features": [ctx.attrs._prep_feature],
            "_plugins": ctx.plugins[FeaturePluginPluginKind],
            "_rootless": ctx.attrs._rootless,
            "_run_container": None,
            "_selected_target_arch": ctx.attrs._target_arch,
            "_working_format": ctx.attrs._working_format,
        }).promise.map(_with_anon_layer)
    else:
        return _with_anon_layer(ctx.attrs.exec_layer)

_genrule_in_image = rule(
    impl = _impl,
    attrs = {
        "bash": attrs.arg(),
        "default_out": attrs.option(attrs.string(), default = None),
        # TODO: exec_layer should probably be the default (which would make this
        # behave more like a standard genrule)
        "exec_layer": attrs.option(
            attrs.exec_dep(providers = [LayerInfo]),
            default = None,
            doc = """
                Layer in which to execute the command, but configure it to build
                for the execution platform instead of the target.
                'exec_layer' must be pre-configured with the prep feature
                (//antlir/antlir2/genrule_in_image:prep)
            """,
        ),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "out": attrs.option(attrs.string(), default = None),
        "outs": attrs.option(attrs.dict(attrs.string(), attrs.string()), default = None),
        "_genrule_in_image": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/genrule_in_image:genrule_in_image")),
        "_layer_analyze_feature": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_depgraph_if:analyze"),
        "_layer_antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
        "_prep_feature": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/genrule_in_image:prep", pulls_plugins = [FeaturePluginPluginKind])),
        "_target_arch": attrs.default_only(attrs.string(
            default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        )),
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = layer_cfg,
    # Because this can instantiate an implicit layer, it must also
    # depend on the feature plugins
    uses_plugins = [FeaturePluginPluginKind],
)

_genrule_in_image_macro = rule_with_default_target_platform(_genrule_in_image)

def genrule_in_image(
        *,
        name: str,
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    default_os = default_os or get_default_os_for_package()
    if rootless == None:
        rootless = get_antlir2_rootless()

    labels = kwargs.pop("labels", [])
    if not rootless:
        labels = selects.apply(labels, lambda labels: list(labels) + ["uses_sudo"])

    _genrule_in_image_macro(
        name = name,
        default_os = default_os,
        rootless = rootless,
        labels = labels,
        exec_compatible_with = ["prelude//platforms:may_run_local"],
        **kwargs
    )
