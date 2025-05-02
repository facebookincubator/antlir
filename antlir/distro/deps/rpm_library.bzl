# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/genrule_in_image:genrule_in_image.bzl", "genrule_in_image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "cpp_binary", "write_file")
load("//antlir/distro/platform:defs.bzl", "default_image_platform")
load(":prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def rpm_library(
        *,
        name: str,
        rpm: str | None = None,
        lib: str | None = None,
        archive: bool = False,
        header_glob = None,
        header_only: bool = False,
        support_linker_l: bool = False,
        visibility: list[str] = ["PUBLIC"],
        compatible_with_os: list[str] = [],
        test_include_headers: list[str] | Select = [],
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
    archive_name = name + ".a"

    genrule_in_image(
        name = "{}--outputs".format(name),
        bash = """
            mkdir "$OUT/headers"

            rpm-library-action \
                --out-headers $OUT/headers \
                {maybe_shared_lib} \
                {maybe_archive} \
                --rpm-name={rpm_name} \
                --lib={lib} \
                --header-glob='{header_globs}'

            {cp_L_dir}
        """.format(
            header_globs = json.encode(header_glob),
            lib = lib,
            rpm_name = rpm,
            soname = soname,
            maybe_archive = "--out-archive=$OUT/{}".format(archive_name) if archive else "",
            maybe_shared_lib = "--out-shared-lib=$OUT/{}".format(soname) if not (header_only or archive) else "",
            cp_L_dir = "mkdir $OUT/L && cp --reflink=auto $OUT/{soname} $OUT/L/ && cp --reflink=auto $OUT/{soname} $OUT/L/lib{soname}".format(soname = soname) if support_linker_l else "",
        ),
        outs = {
            "headers": "headers",
        } | ({
            soname: soname,
        } if not (header_only or archive) else {}) | ({
            archive_name: archive_name,
        } if archive else {}) | ({
            "L": "L",
        } if support_linker_l else {}),
        rootless = True,
        layer = ":{}--layer".format(name),
        target_compatible_with = target_compatible_with,
    )

    prebuilt_cxx_library(
        name = name,
        visibility = visibility,
        header_dirs = [":{}--outputs[headers]".format(name)],
        shared_lib = ":{}--outputs[{}]".format(name, soname) if not (header_only or archive) else None,
        static_lib = ":{}--outputs[{}]".format(name, archive_name) if archive else None,
        header_only = header_only,
        extract_soname = kwargs.pop("extract_soname", not archive),
        exported_linker_flags = ["-L$(location :{}--outputs[L])".format(name)] if support_linker_l else [],
        preferred_linkage = "shared" if not archive else "static",
        target_compatible_with = target_compatible_with,
        labels = [
            "antlir-distro-rpm-library",
        ],
        **kwargs
    )

    write_file(
        name = "{}--test-deps-main.cpp".format(name),
        out = "main.cpp",
        content = selects.apply(
            test_include_headers or [],
            lambda headers: ['#include "{}"'.format(h) for h in headers],
        ) + [
            "int main(int argc, char **argv) {",
            "return 0;",
            "}",
        ],
    )

    cpp_binary(
        name = "{}--test-deps-binary".format(name),
        srcs = [":{}--test-deps-main.cpp".format(name)],
        default_target_platform = default_image_platform(),
        deps = [
            ":" + name,
        ],
    )

    image_sh_test(
        name = "{}--test-deps".format(name),
        test = ":{}--test-deps-binary".format(name),
        layer = "antlir//antlir/distro/deps:base",
        default_target_platform = default_image_platform(),
        rootless = True,
        labels = ["antlir-distro-dep-test"],
        target_compatible_with = target_compatible_with,
    )
