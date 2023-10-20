# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is a buck2 configuration transition that allows us to reconfigure the
target platform for an image based on user-provided attributes, possibly
distinct from the default target platform used by the `buck2 build`.

Currently this supports reconfiguring the target cpu architecture.
"""

load("//antlir/antlir2/bzl/image/facebook:fb_cfg.bzl", "fbcode_platform_refs", "transition_fbcode_platform")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

def _impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch
        if is_facebook:
            constraints = transition_fbcode_platform(refs, attrs, constraints)

    label = platform.label

    # if we made any changes, change the label
    if constraints != platform.configuration.constraints:
        label = "antlir2//layer_transitioned_platform"

    return PlatformInfo(
        label = label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

layer_cfg = transition(
    impl = _impl,
    refs = {
        "arch.aarch64": "ovr_config//cpu/constraints:arm64",
        "arch.x86_64": "ovr_config//cpu/constraints:x86_64",
    } | (
        # @oss-disable
        # @oss-enable {}
    ),
    attrs = [
        # target_arch on image.layer is read to reconfigure for the target cpu
        # arch without having to use -c fbcode.arch
        "target_arch",
    ],
)
