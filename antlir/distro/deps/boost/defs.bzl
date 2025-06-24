# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:hoist.bzl", "hoist")
load("//antlir/bzl:build_defs.bzl", "alias")
load("//antlir/distro/deps:dep_distance_extender.bzl", "dep_distance_extender")
load("//antlir/distro/deps:prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def boost_system_library(
        *,
        name: str,
        header_only: bool = False,
        path: str | Select | None = None,
        exported_deps: list[str] | None = None):
    if not name.startswith("boost"):
        fail("boost library should start with boost")
    if not header_only:
        hoist(
            name = name + ".so",
            layer = ":layer",
            path = path or "/usr/lib64/lib{}.so".format(name),
            rootless = True,
        )
    prebuilt_cxx_library(
        name = name + "--actual",
        exported_headers = {
            "": ":headers",
        },
        shared_lib = ":{}.so".format(name) if not header_only else None,
        preferred_linkage = "shared" if not header_only else None,
        header_namespace = "boost",
        exported_deps = exported_deps,
        visibility = [],
    )
    dep_distance_extender(
        name = name,
        actual = ":{}--actual".format(name),
        visibility = ["PUBLIC"],
    )

    # These aliases are totally useless since CentOS has nothing to do with
    # fbcode, Android or Apple platforms, but it breaks some 'buck2 uquery's and
    # janky macros that append platform suffixes like this
    # Boost uses xplat suffixed targets since it's used in a variety of ecosystems.
    for suffix in ["Fbcode", "Apple", "Android"]:
        alias(
            name = name + suffix,
            actual = ":" + name,
            visibility = ["PUBLIC"],
        )
