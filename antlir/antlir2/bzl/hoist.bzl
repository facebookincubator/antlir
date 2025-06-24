# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")

def _hoist_one(
        ctx: AnalysisContext,
        *,
        path: str,
        out: str,
        is_dir: bool = False) -> Artifact:
    output = ctx.actions.declare_output(out, dir = is_dir)
    script = ctx.actions.write(
        "hoist-{}.sh".format(path.replace("/", "-")),
        cmd_args(
            "#!/bin/bash",
            "set -e",
            cmd_args(
                "sudo" if not ctx.attrs._rootless else cmd_args(),
                "cp",
                "--recursive" if is_dir else cmd_args(),
                "--reflink=auto",
                cmd_args(
                    ctx.attrs.layer[LayerInfo].contents.subvol_symlink,
                    format = "{{}}/{}".format(path.lstrip("/")),
                ),
                output.as_output(),
                delimiter = " ",
            ),
            cmd_args(
                "sudo",
                "chown",
                "--recursive" if is_dir else cmd_args(),
                "$(id -u):$(id -g)",
                output.as_output(),
                delimiter = " ",
            ) if not ctx.attrs._rootless else cmd_args(),
            delimiter = "\n",
        ),
        is_executable = True,
    )
    ctx.actions.run(
        cmd_args(
            script,
            hidden = [output.as_output(), ctx.attrs.layer[LayerInfo].contents.subvol_symlink],
        ),
        category = "hoist",
        identifier = path,
        local_only = True,  # local subvol
    )
    return output

def _impl(ctx: AnalysisContext) -> list[Provider]:
    out = _hoist_one(
        ctx,
        path = ctx.attrs.path,
        out = ctx.attrs.out or paths.basename(ctx.attrs.path) or ctx.attrs.name,
        is_dir = ctx.attrs.dir,
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

def _hoist_many_impl(ctx: AnalysisContext) -> list[Provider]:
    outputs = {
        path: _hoist_one(
            ctx,
            path = path,
            out = paths.basename(path),
            is_dir = ctx.attrs.dirs,
        )
        for path in ctx.attrs.paths
    }

    return [
        DefaultInfo(sub_targets = {
            path: [DefaultInfo(outputs[path])]
            for path in ctx.attrs.paths
        }),
    ]

_hoist_many = rule(
    impl = _hoist_many_impl,
    attrs = {
        "dirs": attrs.bool(default = False),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "paths": attrs.list(attrs.string()),
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = layer_cfg,
)

_hoist_many_macro = rule_with_default_target_platform(_hoist_many)

def _hoist_wrapper(
        *,
        macro: typing.Callable,
        name: str,
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    default_os = default_os or get_default_os_for_package()
    if rootless == None:
        rootless = get_antlir2_rootless()
    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])
    macro(
        name = name,
        default_os = default_os,
        rootless = rootless,
        exec_compatible_with = ["prelude//platforms:may_run_local"],
        **kwargs
    )

hoist = lambda **kwargs: _hoist_wrapper(macro = _hoist_macro, **kwargs)
hoist_many = lambda **kwargs: _hoist_wrapper(macro = _hoist_many_macro, **kwargs)
