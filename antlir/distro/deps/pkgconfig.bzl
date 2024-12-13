# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Use pkg-config to generate a cxx_library target that can be depended on by buck
targets, but will actually be provided by an image.

This uses pkg-config mostly as-is, but is slightly complicated due to how it
gets the information out of an image.

At a high level, here's how this works:
* Create an image layer for this dep
* Install whatever rpm provides 'pkgconfig($lib)'
* Run 'pkg-config' using the image as the sysroot
* Rewrite paths produced by 'pkg-config' to use the relative path prefix of the
root directory

The end result is that this produces a library that can be used without having
to install it into the toolchain image that is shared across all cxx targets
using this toolchain setup.
"""

load("@prelude//:prelude.bzl", "native")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")

PkgconfigLibraryInfo = provider(fields = {
    "cflags": Artifact,
    "libs": Artifact,
})

def _pkgconfig_impl(ctx: AnalysisContext) -> list[Provider]:
    cflags = ctx.actions.declare_output("cflags")
    libs = ctx.actions.declare_output("libs")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._pkgconfig_action[RunInfo],
            cmd_args(ctx.attrs.libname),
            cmd_args(ctx.attrs.root),
            cmd_args(cflags.as_output()),
            cmd_args(libs.as_output()),
        ),
        category = "pkgconfig",
    )

    # associate the flag files with the root directory that they reference,
    # otherwise it will not be materialized on RE
    cflags = cflags.with_associated_artifacts([ctx.attrs.root])
    libs = libs.with_associated_artifacts([ctx.attrs.root])
    return [
        PkgconfigLibraryInfo(
            cflags = cflags,
            libs = libs,
        ),
        DefaultInfo(sub_targets = {
            "cflags": [DefaultInfo(cflags)],
            "libs": [DefaultInfo(libs)],
        }),
    ]

_pkgconfig = rule(
    impl = _pkgconfig_impl,
    attrs = {
        "libname": attrs.string(),
        "root": attrs.source(allow_directory = True),
        "_pkgconfig_action": attrs.default_only(attrs.exec_dep(
            providers = [RunInfo],
            default = "antlir//antlir/distro/deps:pkgconfig-action",
        )),
    },
)

def image_pkgconfig_library(
        *,
        name: str,
        visibility: list[str] = ["PUBLIC"],
        deps: list[str] = [],
        exported_preprocessor_flags: list[str] = [],
        compatible_with_os: list[str] = []):
    # clearly separate out the pkg-config name from the target name in case they
    # ever need to differ (but hopefully they don't)
    pkgconfig_name = name

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

    image.layer(
        name = "{}--layer".format(name),
        features = [
            feature.rpms_install(rpms = [
                # whatever rpm provides this lib config
                "pkgconfig({})".format(pkgconfig_name),
            ]),
        ],
        parent_layer = "antlir//antlir/distro/deps:base",
        rootless = True,
        target_compatible_with = target_compatible_with,
    )
    package.unprivileged_dir(
        name = name + "--root",
        layer = ":{}--layer".format(name),
        rootless = True,
        dot_meta = False,
        target_compatible_with = target_compatible_with,
        visibility = [":{}--pkgconfig".format(name)],
    )

    _pkgconfig(
        name = name + "--pkgconfig",
        libname = pkgconfig_name,
        root = ":{}--root".format(name),
        target_compatible_with = target_compatible_with,
    )

    native.prebuilt_cxx_library(
        name = name,
        visibility = visibility,
        exported_preprocessor_flags = ["@$(location :{}--pkgconfig[cflags])".format(name)] + exported_preprocessor_flags,
        exported_linker_flags = ["@$(location :{}--pkgconfig[libs])".format(name)],
        exported_deps = deps,
        target_compatible_with = target_compatible_with,
    )
