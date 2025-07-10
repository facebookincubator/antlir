# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _transition_impl_with_refs(
        *,
        base_platform: PlatformInfo,
        preserve_constraints: list[ConstraintSettingInfo],
        platform: PlatformInfo) -> PlatformInfo:
    previous_platform_constraints = platform.configuration.constraints
    constraints = dict(base_platform.configuration.constraints)
    for constraint_setting in preserve_constraints:
        prev = previous_platform_constraints.get(constraint_setting.label, None)
        if prev:
            constraints[constraint_setting.label] = prev

    # also preserve anything coming from //antlir/...
    for setting, value in platform.configuration.constraints.items():
        if setting.package == "antlir" or setting.package.startswith("antlir/"):
            constraints[setting] = value

    return PlatformInfo(
        label = base_platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = base_platform.configuration.values,
        ),
    )

def _transition_to_distro_platform_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        TransitionInfo(
            impl = partial(
                _transition_impl_with_refs,
                base_platform = ctx.attrs._distro_platform_base[PlatformInfo],
                preserve_constraints = [dep[ConstraintSettingInfo] for dep in ctx.attrs._preserve_constraints],
            ),
        ),
    ]

transition_to_distro_platform = rule(
    impl = _transition_to_distro_platform_impl,
    attrs = {
        "_distro_platform_base": attrs.default_only(attrs.dep(default = "antlir//antlir/distro/platform:base")),
        # all constraints are dropped except for these which are passed through
        # to the base distro platform above
        "_preserve_constraints": attrs.default_only(attrs.list(
            attrs.dep(providers = [ConstraintSettingInfo]),
            default = [
                "ovr_config//cpu/constraints:cpu",
                "ovr_config//toolchain/python/constraints:python-version",
                "ovr_config//build_mode/constraints:build_mode",
                "ovr_config//build_mode/constraints:core_build_mode",
                # Preserve CUDA project constraint versions.
                "ovr_config//third-party/cuda/constraints:cuda-version",
                "ovr_config//third-party/cudnn/constraints:cudnn-version",
                "ovr_config//third-party/TensorRT/constraints:TensorRT-version",
                "ovr_config//third-party/nccl/constraints:nccl-version",
                # also anything under //antlir gets preserved
            ],
        )),
    },
    is_configuration_rule = True,
)
