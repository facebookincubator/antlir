# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    root = ensure_single_output(ctx.attrs.root)
    cmd = cmd_args(
        ctx.attrs._command_alias[RunInfo],
        cmd_args(root, format = "--root={}"),
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
        "exe": attrs.arg(),
        "labels": attrs.list(attrs.string(), default = []),
        "root": attrs.source(allow_directory = True),
        "_command_alias": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/image_command_alias:command_alias")),
    } | attrs_selected_by_cfg() | cfg_attrs(),
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
