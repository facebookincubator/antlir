# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output(
        ctx.attrs.out or paths.basename(ctx.attrs.path) or ctx.attrs.name,
        dir = ctx.attrs.dir,
    )
    script = ctx.actions.write("hoist.sh", cmd_args(
        "#!/bin/bash",
        "set -e",
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            "cp",
            "--recursive" if ctx.attrs.dir else cmd_args(),
            "--reflink=auto",
            cmd_args(
                ctx.attrs.layer[LayerInfo].contents.subvol_symlink,
                format = "{{}}/{}".format(ctx.attrs.path.lstrip("/")),
            ),
            out.as_output(),
            delimiter = " ",
        ),
        cmd_args(
            "sudo",
            "chown",
            "--recursive" if ctx.attrs.dir else cmd_args(),
            "$(id -u):$(id -g)",
            out.as_output(),
            delimiter = " ",
        ) if not ctx.attrs._rootless else cmd_args(),
        delimiter = "\n",
    ), is_executable = True)
    ctx.actions.run(
        cmd_args(
            script,
            hidden = [out.as_output(), ctx.attrs.layer[LayerInfo].contents.subvol_symlink],
        ),
        category = "hoist",
        local_only = True,  # local subvol
    )
    return [
        DefaultInfo(out),
    ] + ([RunInfo(cmd_args(out))] if ctx.attrs.executable else [])

_hoist = rule(
    impl = _impl,
    attrs = {
        "dir": attrs.bool(default = False),
        "executable": attrs.bool(default = False),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "out": attrs.option(attrs.string(doc = "rename output file"), default = None),
        "path": attrs.string(),
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = layer_cfg,
)

_hoist_macro = rule_with_default_target_platform(_hoist)

def hoist(
        *,
        name: str,
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    if should_all_images_in_package_use_default_os():
        default_os = default_os or get_default_os_for_package()
    if rootless == None:
        rootless = get_antlir2_rootless()
    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])
    _hoist_macro(
        name = name,
        default_os = default_os,
        rootless = rootless,
        **kwargs
    )
