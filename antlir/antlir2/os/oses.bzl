# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:internal_external.bzl", "internal_external", "is_facebook")

arch_t = record(
    name = str,
    select_key = str,
)

def new_arch_t(s: str) -> arch_t:
    return arch_t(
        name = s,
        select_key = {"aarch64": "ovr_config//cpu:arm64", "x86_64": "ovr_config//cpu:x86_64"}[s],
    )

os_t = record(
    name = str,
    architectures = list[arch_t],
    select_key = str,
    flavor = str,
    target = str,
    has_platform_toolchain = bool,
    py_constraint = str,
)

def _new_os(name: str, **kwargs):
    kwargs.setdefault("architectures", internal_external(
        fb = [new_arch_t("x86_64"), new_arch_t("aarch64")],
        oss = [new_arch_t("x86_64")],
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
    # Default to py version 3.12 if we don't know what python version
    py_ver = kwargs.get("py_constraint", "ovr_config//third-party/python/constraints:3.12")
    kwargs.setdefault("py_constraint", py_ver)
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
        py_constraint = "ovr_config//third-party/python/constraints:3.9",
    ),
    _new_os(
        name = "centos10",
        py_constraint = "ovr_config//third-party/python/constraints:3.12",
    ),
]

if is_facebook:
    OSES.extend([
        _new_os(
            name = "eln",
        ),
        # centos8 builds are flaky but it's effectively dead in prod so we don't
        # care and don't need a system toolchain for it (or its rhel8(.8)
        # cousins)
        _new_os(
            name = "centos8",
            architectures = [new_arch_t("x86_64")],
            has_platform_toolchain = False,
        ),
        _new_os(
            name = "rhel8",
            architectures = [new_arch_t("x86_64")],
            has_platform_toolchain = False,
        ),
        _new_os(
            name = "rhel8.8",
            architectures = [new_arch_t("x86_64")],
            has_platform_toolchain = False,
        ),
    ])
else:
    # This is very gross, but there are some tests that still assume C8 exists,
    # even though there are no repos snapshotted for it on GitHub
    OSES.append(
        _new_os(
            name = "centos8",
            architectures = [new_arch_t("x86_64")],
            flavor = "antlir//antlir/antlir2/flavor:none",
            has_platform_toolchain = False,
        ),
    )

# Syntax `tuple[str, ...]` is erroneously declared invalid
# @lint-ignore BUCKFORMAT
def _at_least_centos(release: int) -> tuple[str, ...]:
    match = []
    for os in OSES:
        if os.name == "eln":
            # ELN is basically the newest centos, so it should always match
            match.append(os.select_key)
        if os.name.startswith("centos"):
            this = int(os.name.removeprefix("centos"))
            if this >= release:
                match.append(os.select_key)

    return tuple(match)

os_matchers = struct(
    at_least_centos = _at_least_centos,
)
