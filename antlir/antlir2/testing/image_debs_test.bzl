# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")

def _deb_names_test_impl(ctx: AnalysisContext) -> list[Provider]:
    script = ctx.actions.write(
        "test.sh",
        cmd_args(
            "#!/bin/bash",
            "set -e",
            cmd_args(
                ctx.attrs.image_debs_test[RunInfo],
                cmd_args(ctx.attrs.layer[LayerInfo].facts_db, format = "--facts-db={}"),
                cmd_args("--not-installed") if ctx.attrs.not_installed else cmd_args(),
                ctx.attrs.names,
                delimiter = " ",
            ),
            delimiter = "\n",
        ),
        is_executable = True,
        with_inputs = True,
        has_content_based_path = False,
    )
    return [
        DefaultInfo(),
        RunInfo(cmd_args(script)),
        ExternalRunnerTestInfo(
            type = "simple",
            command = [script],
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                remote_enabled = False,
            ),
        ),
    ]

_deb_names_test = rule(
    impl = _deb_names_test_impl,
    attrs = {
        "image_debs_test": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/testing/image_debs_test:image-debs-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "names": attrs.list(attrs.string()),
        "not_installed": attrs.bool(default = False),
    }
    | cfg_attrs(),
    cfg = layer_cfg,
)

_deb_names_test_macro = rule_with_default_target_platform(_deb_names_test)

def image_test_deb_names(*, default_os: str | None = None, rootless: bool | None = None, **kwargs):
    rootless = rootless if rootless != None else get_antlir2_rootless()
    labels = kwargs.pop("labels", [])
    if not rootless:
        labels.append("uses_sudo")
    kwargs.setdefault(
        "compatible_with",
        ["antlir//antlir/antlir2/os/package_manager:package_manager[apt]"],
    )
    _deb_names_test_macro(default_os = default_os or get_default_os_for_package(), rootless = rootless, labels = labels, **kwargs)
