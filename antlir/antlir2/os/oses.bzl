# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "internal_external", "is_facebook")

arch_t = enum("x86_64", "aarch64")

os_t = record(
    name = str,
    architectures = list[arch_t],
    select_key = str,
    flavor = str,
    target = str,
    has_platform_toolchain = bool,
)

def _new_os(name: str, **kwargs):
    kwargs.setdefault("architectures", internal_external(
        fb = [arch_t("x86_64"), arch_t("aarch64")],
        oss = [arch_t("x86_64")],
    ))
    kwargs.setdefault("select_key", "antlir//antlir/antlir2/os:" + name)
    kwargs.setdefault(
        "flavor",
        internal_external(
            fb = "antlir//antlir/antlir2/facebook/flavor/",
            oss = "//flavor/",
        ) + name + ":" + name,
    )
    kwargs.setdefault("target", "antlir//antlir/antlir2/os:" + name)
    kwargs.setdefault("has_platform_toolchain", True)
    return os_t(
        name = name,
        **kwargs
    )

OSES = [
    _new_os(
        name = "none",
        select_key = "antlir//antlir/antlir2/os:none",
        flavor = "antlir//antlir/antlir2/flavor:none",
        has_platform_toolchain = False,
    ),
    _new_os(
        name = "centos9",
    ),
    _new_os(
        name = "centos10",
    ),
]

if is_facebook:
    OSES.extend([
        _new_os(
            name = "eln",
        ),
        _new_os(
            name = "centos8",
            architectures = [arch_t("x86_64")],
        ),
        _new_os(
            name = "rhel8",
            architectures = [arch_t("x86_64")],
        ),
        _new_os(
            name = "rhel8.8",
            architectures = [arch_t("x86_64")],
        ),
    ])
else:
    # This is very gross, but there are some tests that still assume C8 exists,
    # even though there are no repos snapshotted for it on GitHub
    OSES.append(
        _new_os(
            name = "centos8",
            architectures = [arch_t("x86_64")],
            flavor = None,
        ),
    )
