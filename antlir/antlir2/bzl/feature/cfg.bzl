# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Simple buck2 configuration transition that marks all features as building with
dnf.
"""

load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")

def _impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.default_os:
        constraints = os_transition(
            default_os = attrs.default_os,
            refs = refs,
            constraints = constraints,
            overwrite = False,
        )

    # If there is no package manager configuration, this means we're using the
    # old-style flavor inheritance mechanism which implies dnf
    package_manager_dnf = refs.package_manager_dnf[ConstraintValueInfo]
    if package_manager_dnf.setting.label not in constraints:
        constraints[package_manager_dnf.setting.label] = package_manager_dnf

    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

feature_cfg = transition(
    impl = _impl,
    attrs = ["default_os"],
    refs = {
        "package_manager_dnf": "antlir//antlir/antlir2/os/package_manager:dnf",
    } | os_transition_refs(),
)
