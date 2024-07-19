# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", _cfg_attrs = "cfg_attrs")

load("//antlir/bzl:oss_shim.bzl", fb_cfg_attrs = "empty_dict", fb_refs = "empty_dict", fb_transition = "ret_none") # @oss-enable
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

def cfg_attrs():
    return _cfg_attrs() | rootless_cfg.attrs

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

    constraints = rootless_cfg.transition(
        refs = refs,
        attrs = attrs,
        constraints = constraints,
        overwrite = True,
    )

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
    } | (
        # @oss-disable
        {} # @oss-enable
    ) | rootless_cfg.refs,
    attrs = cfg_attrs().keys(),
)
