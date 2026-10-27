# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbsource//tools/target_determinator/macros:ci.bzl", "ci")
load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

def test_deb(*, name: str, version: str, arch: str = "all", features = [], **kwargs) -> str:
    target_name = name + "_" + version + "_" + arch
    image.layer(
        name = target_name + "--layer",
        features = features,
        rootless = True,
        visibility = [":" + target_name],
    )
    package.deb(name = target_name, layer = ":{}--layer".format(target_name), arch = arch, deb_name = name, version = version, **kwargs)
    return ":" + target_name

def test_on_all_flavors(macro, *, name: str, **kwargs):
    macro(name = name, **kwargs)
    for os in ["debian-trixie", "ubuntu-noble", "ubuntu-stonking"]:
        if os == get_default_os_for_package():
            continue
        alias_kwargs = {}
        # this is the only version that has no arm packages
        if os == "ubuntu-noble":
            alias_kwargs |= {"labels": ci.remove_labels(ci.aarch64()), "target_compatible_with": ["ovr_config//cpu:x86_64"]}
        antlir2_configured_alias(name = name + "-" + os, actual = ":" + name, default_os = os, **alias_kwargs)
