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

python_t = record(
    version_str = str,
    constraint = str,
    interpreter = str,
)

def new_python_t(
    version_str: str | None = None,
    constraint: str | None = None,
    interpreter: str | None = None,
) -> python_t:
    return python_t(
        version_str = version_str or "CPython 3.12",
        constraint = constraint or "ovr_config//third-party/python/constraints:3.12",
        interpreter = interpreter or "python3"
    )

os_t = record(
    name = str,
    architectures = list[arch_t],
    select_key = str,
    flavor = str,
    build_appliance = str | Select,
    target = str,
    has_platform_toolchain = bool,
    python = python_t,
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
    kwargs.setdefault(
        "build_appliance",
        internal_external(
            fb = "antlir//antlir/antlir2/facebook/images/build_appliance/{}:build-appliance".format(name),
            oss = "antlir//flavor/{}:build-appliance".format(name),
        )
    )
    kwargs.setdefault("target", "antlir//antlir/antlir2/os:" + name)
    kwargs.setdefault("has_platform_toolchain", True)
    kwargs.setdefault("python", new_python_t())
    return os_t(
        name = name,
        **kwargs
    )

OSES = [
    _new_os(
        name = "none",
        select_key = "antlir//antlir/antlir2/os:none",
        flavor = "antlir//antlir/antlir2/flavor:none",
        # TODO: this should have its own build_appliance that doesn't have dnf
        # installed, but is not strictly necessary right now
        build_appliance = internal_external(
            fb = "antlir//antlir/antlir2/facebook/images/build_appliance/centos9:build-appliance",
            oss = "//flavor/centos9:build-appliance",
        ),
        has_platform_toolchain = False,
    ),
    _new_os(
        name = "centos9",
        build_appliance = select({
            "DEFAULT": "antlir//antlir/antlir2/facebook/images/build_appliance/centos9:build-appliance",
            "antlir//antlir/antlir2/facebook/flavor/centos9:corp": "antlir//antlir/antlir2/facebook/images/build_appliance/centos9_corp:build-appliance",
        }),
        # This points to the Meta-built third-party/python interpreter.
        python = new_python_t(interpreter = "/usr/local/bin/python3.12")
    ),
    _new_os(
        name = "centos10",
        build_appliance = select({
            "DEFAULT": "antlir//antlir/antlir2/facebook/images/build_appliance/centos10:build-appliance",
            "antlir//antlir/antlir2/facebook/flavor/centos10:corp": "antlir//antlir/antlir2/facebook/images/build_appliance/centos10_corp:build-appliance",
        }),
        # TODO(T238134086): This should point to the third-party/python interpreter when we've verified this correctness.
        python = new_python_t(interpreter = "/usr/bin/python3.12")
    ),
]

if is_facebook:
    OSES.extend([
        _new_os(
            name = "eln",
            has_platform_toolchain = False,
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
        _new_os(
            name = "rhel9",
            architectures = [new_arch_t("x86_64")],
            has_platform_toolchain = False,
        ),
        _new_os(
            name = "rhel9.2",
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

def os_by_name(name: str) -> os_t | None:
    for os in OSES:
        if os.name == name:
            return os
    return None
