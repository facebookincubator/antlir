# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/distro/deps:defs.bzl", "format_select")
load("//antlir/distro/deps:rpm_library.bzl", "rpm_library")

_version_select = select({
    "ovr_config//third-party/cuda/constraints:12.4": "12.4",
    "ovr_config//third-party/cuda/constraints:12.8": "12.8",
})

_dash_version_select = selects.apply(_version_select, lambda v: v.replace(".", "-"))

_arch_select = arch_select(
    aarch64 = "aarch64",
    x86_64 = "x86_64",
)

def _format_attr(attr: typing.Any, dash: bool = False) -> Select:
    return format_select(
        attr,
        version = _dash_version_select if dash else _version_select,
        arch = _arch_select,
    )

def _rpm_library(name, **kwargs) -> None:
    lib = kwargs.pop("lib", None)
    if lib:
        lib = _format_attr(lib)
    header_glob = kwargs.pop("header_glob", None)
    if header_glob:
        header_glob = _format_attr(header_glob)

    rpm_library(
        name = name,
        header_glob = header_glob,
        lib = lib,
        # CUDA RPMs use XX-X as version number instead of XX.X
        rpm = _format_attr(kwargs.pop("rpm"), dash = True),
        # TODO: turn this back to True once the built binaries know how to
        # actually load the cuda libraries
        tests = False,
        **kwargs
    )

cuda = struct(
    rpm_library = _rpm_library,
)
