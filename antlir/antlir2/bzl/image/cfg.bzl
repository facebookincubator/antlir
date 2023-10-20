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
load("//antlir/antlir2/os:defs.bzl", "OsVersionInfo")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

def cfg_attrs():
    return {
        "default_os": attrs.option(attrs.string(), default = None, doc = """
            Reconfigure the layer when no antlir2 os has been set yet, so that
            each intermediate layer can be passed to `buck build` and give a
            reasonable default.
            For more details, see:
            https://www.internalfb.com/intern/staticdocs/antlir2/docs/recipes/multi-os-images/
        """),
        "target_arch": attrs.option(
            attrs.enum(["x86_64", "aarch64"]),
            default = None,
            doc = "Build this image for a specific target arch without using `buck -c`",
        ),
    }

def _impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch
        if is_facebook:
            constraints = transition_fbcode_platform(refs, attrs, constraints)

    if attrs.default_os:
        os = getattr(refs, "os." + attrs.default_os)[OsVersionInfo]
        os_constraint = os.constraint[ConstraintValueInfo]
        family = os.family[ConstraintValueInfo]

        # The rule transition to set the default antlir2 OS only happens if the
        # target has not been configured for a specific OS yet. This way the dep
        # transition takes precedence - in other words, the default_os attribute of
        # the leaf image being built is always respected and reconfigures all layers
        # along the parent_layer chain
        if os_constraint.setting.label not in constraints:
            constraints[os_constraint.setting.label] = os_constraint
            constraints[family.setting.label] = family

    # If a build appliance is being built, we must remove the OS configuration
    # constraint to avoid circular dependencies.
    if attrs.antlir_internal_build_appliance:
        constraints.pop(refs.os_constraint[ConstraintSettingInfo].label, None)
        constraints.pop(refs.os_family_constraint[ConstraintSettingInfo].label, None)

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
        "os.centos8": "//antlir/antlir2/os:centos8",
        "os.centos9": "//antlir/antlir2/os:centos9",
        "os.eln": "//antlir/antlir2/os:eln",
        "os.none": "//antlir/antlir2/os:none",
        "os_constraint": "//antlir/antlir2/os:os",
        "os_family_constraint": "//antlir/antlir2/os/family:family",
    } | (
        # @oss-disable
        # @oss-enable {}
    ),
    attrs = cfg_attrs().keys() + [
        # Build appliances are very low level and cannot depend on a flavor, so
        # they are just not transitioned to an os configuration
        "antlir_internal_build_appliance",
    ],
)

def _remove_os_impl(platform: PlatformInfo, refs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints
    constraints.pop(refs.os_constraint[ConstraintSettingInfo].label, None)
    constraints.pop(refs.os_family_constraint[ConstraintSettingInfo].label, None)
    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

remove_os_constraint = transition(
    impl = _remove_os_impl,
    refs = {
        "os_constraint": "//antlir/antlir2/os:os",
        "os_family_constraint": "//antlir/antlir2/os/family:family",
    },
)
