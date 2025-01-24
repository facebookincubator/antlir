# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:hoist.bzl", "hoist")

def boost_system_library(
        *,
        name: str,
        header_only: bool = False,
        path: str | None = None,
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
    native.prebuilt_cxx_library(
        name = name,
        exported_headers = {
            "": ":headers",
        },
        shared_lib = ":{}.so".format(name) if not header_only else None,
        preferred_linkage = "shared" if not header_only else None,
        header_namespace = "boost",
        exported_deps = exported_deps,
        visibility = ["PUBLIC"],
    )
