# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/genrule_in_image:genrule_in_image.bzl", "genrule_in_image")

def rpm_library(
        *,
        name: str,
        rpm: str | None = None,
        lib: str | None = None,
        header_glob = None,
        header_only: bool = False,
        visibility: list[str] = ["PUBLIC"],
        compatible_with_os: list[str] = [],
        **kwargs):
    """
    Define a cxx_library target that can be used in Buck builds to depend on a
    distro-provided library.

    By default, this tries to be intelligent and automatically extract the
    headers and .so that make the most sense, but there are kwargs that function
    as escape hatches if the automatic determination is wrong.

    # Why not just use pkg-config?
    At first glance, pkg-config seems like an existing tool that does basically
    this. However, it has a number of shortcomings:
        * does not declare headers
        * assumes an entire sysroot (it simply generates compiler flags)
        * does not distinguish between this library and its dependencies

    So, antlir2 provides a simple script (rpm_library_action.py) that attempts
    to determine headers and a shared library (.so) to extract from an rpm.
    """
    target_compatible_with = select({
        "DEFAULT": ["antlir//antlir/distro:incompatible"],
        # pkg-config deps must ONLY be compatible with an antlir2 system toolchain
        "antlir//antlir/distro:build-for-distro": select((
            {"DEFAULT": ["antlir//antlir/distro:incompatible"]} |
            {
                os: []
                for os in compatible_with_os
            }
        ) if compatible_with_os else {"DEFAULT": []}),
    })

    rpm = rpm or (name + "-devel")
    image.layer(
        name = "{}--layer".format(name),
        features = [
            feature.rpms_install(rpms = [rpm]),
        ],
        parent_layer = "antlir//antlir/distro/deps:base",
        rootless = True,
        target_compatible_with = target_compatible_with,
    )

    lib = lib or name
    soname = name + ".so"

    genrule_in_image(
        name = "{}--outputs".format(name),
        bash = """
            mkdir "$OUT/headers"

            rpm-library-action \
                --out-headers $OUT/headers \
                {maybe_shared_lib} \
                --rpm-name={rpm_name} \
                --lib={lib} \
                --header-glob='{header_globs}'
        """.format(
            header_globs = json.encode(header_glob),
            lib = lib,
            rpm_name = rpm,
            soname = soname,
            maybe_shared_lib = "--out-shared-lib=$OUT/{}".format(soname) if not header_only else "",
        ),
        outs = {
            "headers": "headers",
        } | ({
            soname: soname,
        } if not header_only else {}),
        rootless = True,
        layer = ":{}--layer".format(name),
        target_compatible_with = target_compatible_with,
    )

    native.prebuilt_cxx_library(
        name = name,
        visibility = visibility,
        header_dirs = [":{}--outputs[headers]".format(name)],
        shared_lib = ":{}--outputs[{}]".format(name, soname) if not header_only else None,
        header_only = header_only,
        preferred_linkage = "shared",
        target_compatible_with = target_compatible_with,
        labels = ["antlir-distro-rpm-library"],
        **kwargs
    )
