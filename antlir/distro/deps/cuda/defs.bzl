# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/distro/deps:rpm_library.bzl", "rpm_library")

def _selectify_item(item, callable):
    if isinstance(item, list):
        return selects.apply(item, [_selectify_item(i, callable) for i in item])
    return selects.apply(item, callable)

def _selectify_arch(attr):
    if not _contains_template_placeholder(attr, "{arch}"):
        return attr
    return arch_select(
        aarch64 = _selectify_item(attr, lambda a: a.replace("{arch}", "aarch64")),
        x86_64 = _selectify_item(attr, lambda a: a.replace("{arch}", "x86_64")),
    )

def _contains_template_placeholder(attr, placeholder):
    def _helper(val):
        if isinstance(val, list):
            for v in val:
                if _contains_template_placeholder(v, placeholder):
                    return True
            return False
        return placeholder in val

    return selects.test_any_of(attr, _helper)

def _selectify_version(attr):
    if not _contains_template_placeholder(attr, "{version}"):
        return attr
    return select({
        "ovr_config//third-party/cuda/constraints:12.4": _selectify_item(attr, lambda a: a.replace("{version}", "12.4")),
        "ovr_config//third-party/cuda/constraints:12.8": _selectify_item(attr, lambda a: a.replace("{version}", "12.8")),
    })

def _selectify_placeholders(attr):
    return _selectify_arch(_selectify_version(attr))

def cuda_rpm_library(name, **kwargs) -> None:
    lib = kwargs.pop("lib", None)
    if lib:
        lib = _selectify_placeholders(lib)
    header_glob = kwargs.pop("header_glob", None)
    if header_glob:
        header_glob = _selectify_placeholders(header_glob)

    rpm_library(
        name = name,
        header_glob = header_glob,
        lib = lib,
        # CUDA RPMs use XX-X as version number instead of XX.X
        rpm = selects.apply(
            _selectify_placeholders(kwargs.pop("rpm")),
            lambda rpm: rpm.replace("12.", "12-"),
        ),
        **kwargs
    )
