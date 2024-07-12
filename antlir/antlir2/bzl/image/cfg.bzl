# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is a buck2 configuration transition that allows us to reconfigure the
target platform for an image based on user-provided attributes, possibly
distinct from the default target platform used by the `buck2 build`.
"""

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:types.bzl", "FlavorInfo")

load("//antlir/bzl:oss_shim.bzl", fb_cfg_attrs = "empty_dict", fb_refs = "empty_dict", fb_transition = "ret_none") # @oss-enable
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "internal_external", "is_facebook")

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
        "working_format": attrs.option(
            attrs.enum(["btrfs", "overlayfs"]),
            default = None,
            doc = "Underlying on-disk format for the layer build",
        ),
    } | (
        # @oss-disable
        {} # @oss-enable
    ) | rootless_cfg.attrs

def attrs_selected_by_cfg():
    return {
        # only attrs.option because it cannot be set on build appliance layers
        "flavor": attrs.option(
            attrs.dep(providers = [FlavorInfo]),
            default = select({
                # We always need to provide a DEFAULT branch that resolves to
                # None since this is an attrs.option(). When everything is
                # default_os (TODO(T168220644)) and this is not an option, this
                # can be removed.
                "DEFAULT": None,
                "antlir//antlir/antlir2/os:centos9": internal_external(
                    fb = "antlir//antlir/antlir2/facebook/flavor/centos9:centos9",
                    oss = "//flavor/centos9:centos9",
                ),
            } | internal_external(
                fb = {
                    "antlir//antlir/antlir2/os:centos10": "antlir//antlir/antlir2/facebook/flavor/centos10:centos10",
                    "antlir//antlir/antlir2/os:centos8": "antlir//antlir/antlir2/facebook/flavor/centos8:centos8",
                    "antlir//antlir/antlir2/os:eln": "antlir//antlir/antlir2/facebook/flavor/eln:eln",
                    "antlir//antlir/antlir2/os:none": "antlir//antlir/antlir2/flavor:none",
                    "antlir//antlir/antlir2/os:rhel8": "antlir//antlir/antlir2/facebook/flavor/rhel8:rhel8",
                    "antlir//antlir/antlir2/os:rhel8.8": "antlir//antlir/antlir2/facebook/flavor/rhel8.8:rhel8.8",
                },
                oss = {},
            )),
        ),
        "_rootless": rootless_cfg.is_rootless_attr,
        "_working_format": attrs.default_only(attrs.string(
            default = select({
                "DEFAULT": "btrfs",
                "antlir//antlir/antlir2/cfg:btrfs": "btrfs",
                "antlir//antlir/antlir2/cfg:overlayfs": "overlayfs",
            }),
        )),
    }

def _impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch

    if attrs.default_os:
        # The rule transition to set the default antlir2 OS only happens if the
        # target has not been configured for a specific OS yet. This way the dep
        # transition takes precedence - in other words, the default_os attribute
        # of the leaf image being built is always respected and reconfigures all
        # layers along the parent_layer chain
        constraints = os_transition(
            default_os = attrs.default_os,
            refs = refs,
            constraints = constraints,
            overwrite = False,
        )

    # If there is still no package manager configuration, this means we're using
    # the old-style flavor inheritance mechanism which implies dnf
    package_manager_dnf = refs.package_manager_dnf[ConstraintValueInfo]
    if package_manager_dnf.setting.label not in constraints:
        constraints[package_manager_dnf.setting.label] = package_manager_dnf

    constraints = rootless_cfg.transition(refs = refs, attrs = attrs, constraints = constraints)

    if is_facebook:
        constraints = fb_transition(refs, attrs, constraints, overwrite = False)

    working_format_setting = refs.working_format[ConstraintSettingInfo]
    if attrs.working_format and working_format_setting.label not in constraints:
        constraints[working_format_setting.label] = getattr(refs, "working_format." + attrs.working_format)[ConstraintValueInfo]

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
        "package_manager_constraint": "antlir//antlir/antlir2/os/package_manager:package_manager",
        "package_manager_dnf": "antlir//antlir/antlir2/os/package_manager:dnf",
        "rooted": "antlir//antlir/antlir2/antlir2_rootless:rooted",
        "rootless": "antlir//antlir/antlir2/antlir2_rootless:rootless",
        "working_format": "antlir//antlir/antlir2/cfg:working_format",
        "working_format.btrfs": "antlir//antlir/antlir2/cfg:btrfs",
        "working_format.overlayfs": "antlir//antlir/antlir2/cfg:overlayfs",
    } | (
        # @oss-disable
        {} # @oss-enable
    ) | os_transition_refs(),
    attrs = cfg_attrs().keys(),
)
