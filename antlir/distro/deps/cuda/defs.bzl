# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/distro/deps:rpm_library.bzl", "rpm_library")

def _select_arch(attr):
    return arch_select(
        aarch64 = selects.apply(attr, lambda a: _replace(a, "{arch}", "aarch64")),
        x86_64 = selects.apply(attr, lambda a: _replace(a, "{arch}", "x86_64")),
    )

def _replace(attr: typing.Any, kw: str, replacement: str) -> typing.Any:
    if isinstance(attr, list):
        return [_replace(i, kw, replacement) for i in attr]
    if isinstance(attr, tuple):
        return tuple([_replace(i, kw, replacement) for i in attr])
    return attr.replace(kw, replacement)

def _select_version(attr: typing.Any, dash: bool = False) -> Select:
    def _format_version(version: str) -> str:
        return version.replace(".", "-") if dash else version

    return select({
        "ovr_config//third-party/cuda/constraints:12.4": selects.apply(attr, lambda a: _replace(a, "{version}", _format_version("12.4"))),
        "ovr_config//third-party/cuda/constraints:12.8": selects.apply(attr, lambda a: _replace(a, "{version}", _format_version("12.8"))),
    })

def _select_template(attr, dash: bool = False):
    return _select_arch(_select_version(attr, dash = dash))

def _rpm_library(name, **kwargs) -> None:
    lib = kwargs.pop("lib", None)
    if lib:
        lib = _select_template(lib)
    header_glob = kwargs.pop("header_glob", None)
    if header_glob:
        header_glob = _select_template(header_glob)

    rpm_library(
        name = name,
        header_glob = header_glob,
        lib = lib,
        # CUDA RPMs use XX-X as version number instead of XX.X
        rpm = _select_template(kwargs.pop("rpm"), dash = True),
        # TODO: turn this back to True once the built binaries know how to
        # actually load the cuda libraries
        tests = False,
        **kwargs
    )

cuda = struct(
    rpm_library = _rpm_library,
    select_version = _select_version,
)
