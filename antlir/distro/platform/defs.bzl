# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
load("@prelude//:rules.bzl", "platform")
load("//antlir/antlir2/os:oses.bzl", "OSES", "arch_t", "new_arch_t", "os_t")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")

def _cpu_label(arch: arch_t, *, constraint: bool = False) -> str:
    sk = arch.select_key
    if constraint:
        sk = sk.replace("ovr_config//cpu:", "ovr_config//cpu/constraints:")
    return sk

def _image_platform(
        *,
        name: str,
        os: os_t,
        arch: arch_t):
    platform(
        name = name,
        constraint_values = [
            _cpu_label(arch, constraint = True),
        ],
        visibility = ["PUBLIC"],
        deps = [
            os.target,
            "antlir//antlir/distro/platform:base",
        ],
    )

def _platform_name(os: os_t, arch: arch_t) -> str:
    return os.name + "-" + arch.name

def define_platforms():
    for os in OSES:
        if not os.has_platform_toolchain:
            continue
        for arch in os.architectures:
            _image_platform(
                name = _platform_name(os, arch),
                arch = arch,
                os = os,
            )

def _os_label(os: os_t) -> str:
    return "antlir//antlir/antlir2/os:" + os.name

def _platform_label(os: os_t, arch: arch_t) -> str:
    return "antlir//antlir/distro/platform:" + _platform_name(os, arch)

def alias_for_current_image_platform(*, name: str, actual: str):
    """
    Configure another target (typically a binary rule) to build against the
    antlir2 system platform for whatever configuration is currently active - in
    other words, build a binary against the system platform for an image in
    which the binary is being installed.
    """
    tcw = {"DEFAULT": ["antlir//antlir/distro:incompatible"]}
    platform = {}
    for os in OSES:
        if not os.has_platform_toolchain:
            continue
        os_tcw = {"DEFAULT": ["antlir//antlir/distro:incompatible"]}
        os_plat = {}
        for arch in os.architectures:
            os_plat[_cpu_label(arch)] = _platform_label(os, arch)
            os_tcw[_cpu_label(arch)] = []

        platform[_os_label(os)] = select(os_plat)
        tcw[_os_label(os)] = select(os_tcw)

    native.configured_alias(
        name = name,
        actual = actual,
        target_compatible_with = select(tcw),
        platform = select(platform),
    )

def default_image_platform(os: str | None = None):
    os = os or get_default_os_for_package()
    # @oss-disable
    default_arch = "aarch64" if native.host_info().arch.is_aarch64 else "x86_64" # @oss-enable

    default_arch = new_arch_t(default_arch)
    found_os = None
    for test_os in OSES:
        if test_os.name == os:
            found_os = test_os
            break
    if not found_os:
        fail("no known os '{}'".format(os))
    return _platform_label(found_os, default_arch)
