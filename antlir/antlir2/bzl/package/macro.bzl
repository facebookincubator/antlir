# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("@fbsource//tools/target_determinator/macros:ci.bzl", "ci")
load("@prelude//utils:expect.bzl", "expect")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")
# @oss-disable[end= ]: load("//antlir/antlir2/os/facebook:package.bzl", "get_default_rou_for_package")
load("//antlir/bzl:build_defs.bzl", "get_visibility")

def package_macro(
        buck_rule,
        *,
        always_needs_root: bool = False,
        always_rootless: bool = False):
    def _inner(
            default_os: str | None = None,
            rootless: bool | None = None,
            # @oss-disable[end= ]: default_rou: str | None = None,
            **kwargs):
        visibility = get_visibility(kwargs.pop("visibility", []))

        # get_default_os_for_package reads the closest PACKAGE file, it has
        # nothing to do with antlir2 output packages
        default_os = default_os or get_default_os_for_package()
        if always_needs_root:
            kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])
            expect(rootless != True, "this package type always needs root, remove this flag since it does not work")
            rootless = False
            kwargs["target_compatible_with"] = kwargs.pop("target_compatible_with", []) + [rootless_cfg.refs["rooted"]]
        elif rootless == None:
            rootless = get_antlir2_rootless()
        if always_rootless:
            rootless = True

        labels = kwargs.pop("labels", [])
        additional_labels = []
        # @oss-disable[end= ]: additional_labels += ci.labels(ci.remove(ci.windows(), ci.mode("fbcode//mode/opt-win")))
        if not rootless:
            additional_labels += ["uses_sudo"]
        labels = selects.apply(labels, lambda labels: additional_labels + list(labels))

        # Package actions use local_only=True, but add it as a constraint on the
        # exec platform too so that aarch64 devservers don't try to run it
        # remotely anyway
        kwargs.setdefault("exec_compatible_with", ["prelude//platforms:may_run_local"])

        buck_rule(
            default_os = default_os,
            # @oss-disable[end= ]: default_rou = default_rou or get_default_rou_for_package(),
            rootless = rootless,
            labels = labels,
            visibility = visibility,
            **(default_target_platform_kwargs() | kwargs)
        )

    return _inner
