# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "alias", "cpp_binary", "write_file")
load("//antlir/distro/platform:defs.bzl", "default_image_platform")
load(":dep_distance_extender.bzl", "dep_distance_extender")
load(":prebuilt_cxx_library.bzl", "prebuilt_cxx_library")

def _rpm_library_action_impl(ctx: AnalysisContext) -> list[Provider]:
    sub_targets = {}
    headers = ctx.actions.declare_output("headers", dir = True)
    sub_targets["headers"] = [DefaultInfo(headers)]

    cmd = cmd_args(
        ctx.attrs._rpm_library_action[RunInfo],
        cmd_args(ctx.attrs.layer[LayerInfo].contents.subvol_symlink, format = "--root={}"),
        cmd_args(ctx.attrs.lib, format = "--lib={}"),
        cmd_args(ctx.attrs.rpm_name, format = "--rpm-name={}"),
        cmd_args(ctx.attrs.soname, format = "--soname={}"),
        cmd_args(headers.as_output(), format = "--out-headers={}"),
    )

    if ctx.attrs.support_linker_l:
        L = ctx.actions.declare_output("L", dir = True)
        sub_targets["L"] = [DefaultInfo(L)]
        cmd.add(cmd_args(L.as_output(), format = "--out-L={}"))

    if not (ctx.attrs.header_only or ctx.attrs.archive):
        lib = ctx.actions.declare_output(ctx.attrs.soname)
        sub_targets[ctx.attrs.soname] = [DefaultInfo(lib)]
        cmd.add(cmd_args(lib.as_output(), format = "--out-shared-lib={}"))

    if ctx.attrs.archive:
        archive = ctx.actions.declare_output(ctx.attrs.archive_name)
        sub_targets[ctx.attrs.archive_name] = [DefaultInfo(archive)]
        cmd.add(cmd_args(archive.as_output(), format = "--out-archive={}"))

    if ctx.attrs.header_glob:
        header_glob = []
        for tup in ctx.attrs.header_glob:
            header_glob.extend(tup)
        cmd.add(cmd_args(header_glob, format = "--header-glob={}"))

    if ctx.attrs.headers:
        if isinstance(ctx.attrs.headers, dict):
            extract_headers = ["{}={}".format(h, f) for h, f in ctx.attrs.headers.items()]
        else:
            extract_headers = ctx.attrs.headers
        cmd.add(cmd_args(extract_headers, format = "--header={}"))

    ctx.actions.run(cmd, category = "rpm_library", local_only = True)
    return [
        DefaultInfo(sub_targets = sub_targets),
    ]

_rpm_library_action = rule(
    impl = _rpm_library_action_impl,
    attrs = {
        "archive": attrs.bool(),
        "archive_name": attrs.string(),
        "header_glob": attrs.option(attrs.list(attrs.tuple(attrs.string(), attrs.string())), default = None),
        "header_only": attrs.bool(),
        "headers": attrs.option(
            attrs.one_of(
                attrs.list(attrs.string()),
                attrs.dict(attrs.string(), attrs.string()),
            ),
            default = None,
        ),
        "layer": attrs.dep(providers = [LayerInfo]),
        "lib": attrs.string(),
        "rpm_name": attrs.one_of(attrs.string(), attrs.list(attrs.string())),
        "soname": attrs.string(),
        "support_linker_l": attrs.bool(),
        "_rpm_library_action": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/distro/deps:rpm-library-action")),
    },
)

_rpm_library_action_macro = rule_with_default_target_platform(_rpm_library_action)

def rpm_library(
        *,
        name: str,
        rpm: str | Select | list[str] | None = None,
        lib: str | Select | None = None,
        archive: bool = False,
        headers: list[str] | dict[str, str] | None = None,
        header_glob = None,
        header_only: bool = False,
        support_linker_l: bool = False,
        visibility: list[str] = ["PUBLIC"],
        compatible_with_os: list[str] = [],
        test_include_headers: list[str] | Select = [],
        dnf_additional_repos: list[str] | None = None,
        test_deps_parent_layer: str | None = None,
        tests: bool = True,
        layer: Select | str | None = None,
        labels: list[str] | None = None,
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
    subjects = selects.apply(
        rpm,
        lambda rpm: [rpm] if isinstance(rpm, str) else rpm,
    )

    if not layer:
        layer = "{}--layer".format(name)
        image.layer(
            name = layer,
            features = [
                feature.rpms_install(subjects = subjects),
            ],
            parent_layer = "antlir//antlir/distro/deps:base",
            rootless = True,
            target_compatible_with = target_compatible_with,
            dnf_additional_repos = dnf_additional_repos,
        )
        layer = ":" + layer

    lib = lib or name
    soname = name + ".so"
    archive_name = name + ".a"

    _rpm_library_action_macro(
        name = "{}--outputs".format(name),
        lib = lib,
        soname = soname,
        header_only = header_only,
        archive_name = archive_name,
        archive = archive,
        header_glob = header_glob,
        headers = headers,
        rpm_name = rpm,
        support_linker_l = support_linker_l,
        layer = layer,
        target_compatible_with = target_compatible_with,
    )

    exported_linker_flags = kwargs.pop("exported_linker_flags", [])
    if support_linker_l:
        exported_linker_flags.append("-L$(location :{}--outputs[L])".format(name))

    prebuilt_cxx_library(
        name = name + "--actual",
        header_dirs = [":{}--outputs[headers]".format(name)],
        shared_lib = ":{}--outputs[{}]".format(name, soname) if not (header_only or archive) else None,
        static_lib = ":{}--outputs[{}]".format(name, archive_name) if archive else None,
        header_only = header_only,
        extract_soname = kwargs.pop("extract_soname", not archive),
        exported_linker_flags = exported_linker_flags,
        preferred_linkage = "shared" if not archive else "static",
        target_compatible_with = target_compatible_with,
        labels = labels or [
            "antlir-distro-rpm-library",
        ],
        visibility = [],
        **kwargs
    )
    dep_distance_extender(
        name = name,
        actual = ":" + name + "--actual",
        target_compatible_with = target_compatible_with,
        visibility = visibility,
    )

    # These aliases are totally useless since CentOS has nothing to do with
    # fbcode, Android or Apple platforms, but it breaks some 'buck2 uquery's and
    # janky macros that append platform suffixes like this
    for suffix in ["Fbcode", "Apple", "Android"]:
        alias(
            name = name + suffix,
            actual = ":" + name,
            target_compatible_with = target_compatible_with,
            visibility = ["PUBLIC"],
        )

    if not tests:
        return

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
            ":{}--actual".format(name),
        ],
    )

    image.layer(
        name = "{}--test-deps-layer".format(name),
        features = [
            feature.install(
                src = ":{}--test-deps-binary".format(name),
                dst = "/test-deps-binary",
                transition_to_distro_platform = True,
            ),
            feature.rpms_install(rpms = ["/bin/sh"]),  # need shell to invoke the test
        ],
        dnf_additional_repos = dnf_additional_repos,
        parent_layer = test_deps_parent_layer,
    )

    image_sh_test(
        name = "{}--test-deps".format(name),
        test = "antlir//antlir/distro/deps:test-deps-binary",
        layer = ":{}--test-deps-layer".format(name),
        default_target_platform = default_image_platform(),
        rootless = True,
        labels = ["antlir-distro-dep-test"],
        target_compatible_with = target_compatible_with,
    )
