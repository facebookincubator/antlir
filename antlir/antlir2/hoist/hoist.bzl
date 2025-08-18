# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", paths_bzl = "paths")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

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
                outputs[path] = ctx.actions.declare_output(path.removeprefix("/"))
    else:
        default_output = ctx.actions.declare_output(
            ctx.attrs.out or paths_bzl.basename(ctx.attrs.path),
            dir = ctx.attrs.dir,
        )
        outputs = {ctx.attrs.path: default_output}
        projected_outputs = {}
    for path, output in outputs.items():
        ctx.actions.run(
            cmd_args(
                "sudo" if not ctx.attrs.layer[LayerInfo].contents.subvol_symlink_rootless else cmd_args(),
                ctx.attrs._hoist[RunInfo],
                cmd_args(ctx.attrs.layer[LayerInfo].contents.subvol_symlink, format = "--subvol-symlink={}"),
                "--rootless" if ctx.attrs.layer[LayerInfo].contents.subvol_symlink_rootless else cmd_args(),
                cmd_args(path, format = "--path={}"),
                cmd_args(output.as_output(), format = "--out={}"),
            ),
            category = "hoist",
            identifier = path,
            allow_cache_upload = True,
            local_only = True,  # needs local subvol
        )

    providers = [
        DefaultInfo(default_output, sub_targets = {
            path: [DefaultInfo(output)]
            for path, output in (outputs | projected_outputs).items()
        }),
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
    },
)

_hoist_macro = rule_with_default_target_platform(_hoist)

def hoist(**kwargs):
    kwargs.setdefault("exec_compatible_with", ["prelude//platforms:may_run_local"])
    _hoist_macro(**kwargs)
