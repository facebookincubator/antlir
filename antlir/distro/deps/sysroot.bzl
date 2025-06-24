# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:hoist.bzl", "hoist")
load(":dep_distance_extender.bzl", "dep_distance_extender")
load(":prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def sysroot_dep(
        *,
        name: str,
        archive: bool = False,
        lib: str | None = None,
        visibility: list[str] = ["PUBLIC"],
        extract_soname: bool | None = None,
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
        target_compatible_with = kwargs.get("target_compatible_with"),
    )

    do_extract_soname = not archive
    if extract_soname != None:
        do_extract_soname = extract_soname

    prebuilt_cxx_library(
        name = name + "--actual",
        shared_lib = (":" + lib) if not archive else None,
        static_lib = (":" + lib) if archive else None,
        preferred_linkage = "shared" if not archive else "static",
        extract_soname = do_extract_soname,
        labels = ["antlir-distro-dep"],
        visibility = [],
        **kwargs
    )
    dep_distance_extender(
        name = name,
        actual = ":" + name + "--actual",
        visibility = visibility,
    )
