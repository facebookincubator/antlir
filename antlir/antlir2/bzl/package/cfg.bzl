# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", _cfg_attrs = "cfg_attrs")
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

cfg_attrs = _cfg_attrs

# Let the layer be configured by the same configuration attrs in image.layer
layer_attrs = {
    "layer": attrs.dep(providers = [LayerInfo]),
} | cfg_attrs()

def _package_cfg_impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch

    if attrs.default_os:
        constraints = os_transition(
            default_os = attrs.default_os,
            refs = refs,
            constraints = constraints,
        )

    rootless = refs.rootless[ConstraintValueInfo]
    if attrs.rootless != None:
        if attrs.rootless:
            constraints[rootless.setting.label] = rootless
        else:
            constraints[rootless.setting.label] = refs.rooted[ConstraintValueInfo]
    elif rootless.setting.label not in constraints:
        # The default is rooted image builds. This is not strictly necessary,
        # but does make it easier to `buck2 audit configurations` when debugging
        # any failures
        constraints[rootless.setting.label] = refs.rooted[ConstraintValueInfo]

    if is_facebook:
        constraints = fb_transition(
            refs,
            attrs,
            constraints,
        )

    label = platform.label

    # if we made any changes, change the label
    if constraints != platform.configuration.constraints:
        label = "antlir2//packaged_transitioned_platform"

    return PlatformInfo(
        label = label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

package_cfg = transition(
    impl = _package_cfg_impl,
    refs = os_transition_refs() | {
        "arch.aarch64": "ovr_config//cpu/constraints:arm64",
        "arch.x86_64": "ovr_config//cpu/constraints:x86_64",
        "rooted": antlir2_dep("//antlir/antlir2/antlir2_rootless:rooted"),
        "rootless": antlir2_dep("//antlir/antlir2/antlir2_rootless:rootless"),
    } | (
        # @oss-disable
        {} # @oss-enable
    ),
    attrs = cfg_attrs().keys(),
)
