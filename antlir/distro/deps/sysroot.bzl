# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:hoist.bzl", "hoist")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load(":dep_distance_extender.bzl", "dep_distance_extender")
load(":prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def export_from_sysroot(name: str, path: str | Select, visibility = ["PUBLIC"], **kwargs):
    """
    Export a particular path from the sysroot to refer to it in rules.
    """
    hoist(
        name = name,
        path = path,
        layer = "antlir//antlir/distro/toolchain/cxx:sysroot-layer",
        visibility = visibility,
        **kwargs
    )

def sysroot_dep(
        *,
        name: str,
        archive: bool = False,
        lib: str | Select | None = None,
        header_dirs: list[str] | Select = [],
        visibility: list[str] = ["PUBLIC"],
        extract_soname: bool | None = None,
        **kwargs):
    """
    A cxx_library target that exposes a library that exists in the sysroot.
    """
    lib = lib or ("lib" + name + (".a" if archive else ".so"))

    hoist(
        name = name + "-lib",
        layer = "antlir//antlir/distro/toolchain/cxx:sysroot-layer",
        path = selects.apply(lib, lambda l: l if paths.is_absolute(l) else paths.join("/usr/lib64", l)),
        visibility = [],
    )

    if header_dirs:
        hoist(
            name = name + "-headers",
            layer = "antlir//antlir/distro/toolchain/cxx:sysroot-layer",
            paths = selects.apply(
                header_dirs,
                lambda header_dirs: [
                    header_dir if paths.is_absolute(header_dir) else paths.join("/usr", header_dir)
                    for header_dir in header_dirs
                ],
            ),
            visibility = [],
            target_compatible_with = kwargs.get("target_compatible_with"),
        )
        kwargs["header_dirs"] = selects.apply(
            header_dirs,
            lambda header_dirs: [":{}-headers[{}]".format(name, header_dir) for header_dir in header_dirs],
        )

    do_extract_soname = not archive
    if extract_soname != None:
        do_extract_soname = extract_soname

    prebuilt_cxx_library(
        name = name + "--actual",
        shared_lib = (":{}-lib".format(name)) if not archive else None,
        static_lib = (":{}-lib".format(name)) if archive else None,
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
