# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    root = ensure_single_output(ctx.attrs.root)
    if ctx.attrs.env:
        env_json = ctx.actions.write_json("env.json", ctx.attrs.env)
    else:
        env_json = None
    cmd = cmd_args(
        ctx.attrs._command_alias[RunInfo],
        cmd_args(root, format = "--root={}"),
        cmd_args(env_json, format = "--env={}") if env_json else cmd_args(),
        cmd_args(ctx.attrs.pass_env, format = "--pass-env={}"),
        "--single-user-userns" if ctx.attrs.single_user_userns else cmd_args(),
        "--",
        ctx.attrs.exe,
        cmd_args(ctx.attrs.args),
    )
    script = ctx.actions.declare_output("script.sh")
    script, hidden = ctx.actions.write(
        script,
        cmd_args(
            "#!/usr/bin/env bash",
            "set -e",
            '__SRC="${BASH_SOURCE[0]}"',
            '__SRC="$(realpath "$__SRC")"',
            '__SCRIPT_DIR=$(dirname "$__SRC")',
            cmd_args("exec", cmd, '"$@"', delimiter = " \\\n  "),
            "",
            delimiter = "\n",
            relative_to = (script, 1),
            absolute_prefix = '"$__SCRIPT_DIR/"',
        ),
        with_inputs = True,
        allow_args = True,
        is_executable = True,
    )
    return [
        DefaultInfo(script),
        RunInfo(args = cmd_args(script, hidden = hidden)),
    ]

_image_command_alias = rule(
    impl = _impl,
    attrs = {
        "args": attrs.list(attrs.arg(), default = []),
        "env": attrs.dict(attrs.string(), attrs.arg(), default = {}),
        "exe": attrs.arg(),
        "labels": attrs.list(attrs.string(), default = []),
        "pass_env": attrs.list(attrs.string(), default = []),
        "root": attrs.source(allow_directory = True),
        "single_user_userns": attrs.bool(
            default = False,
            doc = """
                If set, don't unshare into a fully remapped antlir userns, just
                unshare and map the current (ug)id to root and hope that it's
                good enough for what we're going to do (it usually is)
            """,
        ),
        "_command_alias": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/image_command_alias:command_alias")),
    } | cfg_attrs(),
    cfg = layer_cfg,
)

_image_command_alias_macro = rule_with_default_target_platform(_image_command_alias)

def image_command_alias(
        *,
        name: str,
        layer: str | None = None,
        root: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    if layer:
        if root:
            fail("'layer' and 'root' are mutually exclusive")
        package.unprivileged_dir(
            name = name + "--root",
            layer = layer,
            dot_meta = False,
            rootless = rootless,
            visibility = [":" + name],
        )
        root = ":{}--root".format(name)

    labels = kwargs.pop("labels", [])
    if not rootless:
        labels = selects.apply(labels, lambda labels: list(labels) + ["uses_sudo"])

    _image_command_alias_macro(
        name = name,
        root = root,
        rootless = rootless,
        labels = labels,
        # This is always going to be used as an 'exec_dep' so we need this to
        # force the chosen execution platform to actually have a cpu
        # architecture so we know how to run the image
        compatible_with = kwargs.pop("compatible_with", [
            "ovr_config//cpu:arm64",
            "ovr_config//cpu:x86_64",
        ]),
        **kwargs
    )
