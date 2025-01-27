# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:hoist.bzl", "hoist")
load(":prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def sysroot_dep(
        *,
        name: str,
        archive: bool = False,
        lib: str | None = None,
        visibility: list[str] = ["PUBLIC"],
        **kwargs):
    """
    A cxx_library target that exposes a library that exists in the sysroot.
    """
    lib = lib or ("lib" + name + (".a" if archive else ".so"))

    hoist(
        name = lib,
        layer = "antlir//antlir/distro/deps:sysroot-layer",
        path = "/usr/lib64/" + lib,
        rootless = True,
        visibility = [],
    )

    prebuilt_cxx_library(
        name = name,
        shared_lib = ":" + lib,
        preferred_linkage = "shared",
        visibility = visibility,
        labels = ["antlir-distro-dep"],
        **kwargs
    )
