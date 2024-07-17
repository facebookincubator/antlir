# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:expect.bzl", "expect")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")
load("//antlir/bzl/build_defs.bzl", "get_visibility")

def package_macro(
        buck_rule,
        *,
        always_needs_root: bool = False,
        always_rootless: bool = False):
    def _inner(
            use_default_os_from_package: bool | None = None,
            default_os: str | None = None,
            rootless: bool | None = None,
            **kwargs):
        visibility = get_visibility(kwargs.pop("visibility", []))

        if use_default_os_from_package == None:
            use_default_os_from_package = should_all_images_in_package_use_default_os()
        if use_default_os_from_package:
            # get_default_os_for_package reads the closest PACKAGE file, it has
            # nothing to do with antlir2 output packages
            default_os = default_os or get_default_os_for_package()
        if always_needs_root:
            kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])
            expect(rootless != True, "this package type always needs root, remove this flag since it does not work")
            rootless = False
            kwargs["compatible_with"] = kwargs.pop("compatible_with", []) + [rootless_cfg.refs["rooted"]]
        elif rootless == None:
            rootless = get_antlir2_rootless()
        if always_rootless:
            rootless = True
        buck_rule(
            default_os = default_os,
            rootless = rootless,
            visibility = visibility,
            **(kwargs | default_target_platform_kwargs())
        )

    return _inner
