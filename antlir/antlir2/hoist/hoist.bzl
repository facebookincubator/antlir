# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", paths_bzl = "paths")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs")
load("//antlir/antlir2/bzl/package:cfg.bzl", "package_cfg")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    if int(bool(ctx.attrs.path)) + int(bool(ctx.attrs.paths)) != 1:
        fail("exactly one of path or paths must be set")

    if ctx.attrs.paths:
        if ctx.attrs.out:
            fail("out cannot be set in combination with multiple paths")
        if ctx.attrs.dir:
            fail("dir cannot be set in combination with multiple paths")
        default_output = None
        paths = sorted(ctx.attrs.paths, key = lambda path: path.count("/"))
        outputs = {}
        projected_outputs = {}
        for path in paths:
            candidate = path
            projected = False
            for _ in range(0, path.count("/")):
                candidate = paths_bzl.dirname(candidate)
                if candidate in outputs or candidate in projected_outputs:
                    projected_outputs[path] = outputs.get(candidate, projected_outputs.get(candidate)).project(paths_bzl.relativize(path, candidate))
                    projected = True
                    break
            if not projected:
                outputs[path] = ctx.actions.declare_output(path.removeprefix("/"), has_content_based_path = False)
    else:
        default_output = ctx.actions.declare_output(
            ctx.attrs.out or paths_bzl.basename(ctx.attrs.path),
            dir = ctx.attrs.dir,
            has_content_based_path = False,
        )
        outputs = {ctx.attrs.path: default_output}
        projected_outputs = {}
    working_format = ctx.attrs._working_format
    if working_format == "btrfs":
        layer_args = cmd_args(ctx.attrs.layer[LayerInfo].contents.subvol_symlink, format = "--subvol-symlink={}")
        rootless = ctx.attrs.layer[LayerInfo].contents.subvol_symlink_rootless
    else:
        layer_args = cmd_args(ctx.attrs.layer[LayerInfo].contents.cad_stack, format = "--layer={}")
        rootless = ctx.attrs._rootless
    for path, output in outputs.items():
        ctx.actions.run(
            cmd_args(
                "sudo" if not rootless else cmd_args(),
                ctx.attrs._hoist[RunInfo],
                layer_args,
                cmd_args(working_format, format = "--working-format={}"),
                "--rootless" if rootless else cmd_args(),
                cmd_args(path, format = "--path={}"),
                cmd_args(output.as_output(), format = "--out={}"),
            ),
            category = "hoist",
            identifier = path,
            allow_cache_upload = True,
            # btrfs subvolumes can only exist locally, and there is no sudo on RE
            local_only = (working_format == "btrfs") or (not rootless),
        )

    providers = [
        DefaultInfo(default_output, sub_targets = {path: [DefaultInfo(output)] for path, output in (outputs | projected_outputs).items()}),
    ]

    if ctx.attrs.executable:
        if ctx.attrs.dir:
            fail("executable=True cannot be combined with dir=True")
        providers.append(RunInfo(cmd_args(default_output)))

    return providers

_hoist = rule(
    impl = _impl,
    attrs = {
        "dir": attrs.bool(default = False),
        "executable": attrs.bool(default = False),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "out": attrs.option(attrs.string(), default = None),
        "path": attrs.option(attrs.string(), default = None),
        "paths": attrs.option(attrs.list(attrs.string()), default = None),
        "_hoist": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/hoist:hoist")),
        "_rootless": attrs_selected_by_cfg["_rootless"],
        "_working_format": attrs_selected_by_cfg["_working_format"],
    }
    | cfg_attrs(),
    cfg = package_cfg,
)

_hoist_macro = rule_with_default_target_platform(_hoist)

def hoist(**kwargs):
    kwargs.setdefault("exec_compatible_with", ["prelude//platforms:runs_local[may_run_local]"])
    _hoist_macro(**kwargs)
