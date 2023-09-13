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

def _impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    if not attrs.target_arch:
        return platform
    target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]

    # The rule transition only happens if the target has not been configured for
    # a specific centos yet. This way the dep transition takes precedence.
    constraints = platform.configuration.constraints
    constraints = {
        k: v
        for k, v in constraints.items()
        if k != target_arch.setting.label
    }
    constraints[target_arch.setting.label] = target_arch

    label = platform.label
    if not label.endswith(".layer"):
        label += ".layer"

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
    },
    attrs = [
        # target_arch on image.layer is read to reconfigure for the target cpu
        # arch without having to use -c fbcode.arch
        "target_arch",
    ],
)
