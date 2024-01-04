# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is a buck2 configuration transition that allows us to reconfigure the
target platform for an image based on user-provided attributes, possibly
distinct from the default target platform used by the `buck2 build`.
"""

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:types.bzl", "FlavorInfo")
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs", "remove_os_constraints")
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
        "rootless": attrs.option(attrs.bool(), default = None),
        "target_arch": attrs.option(
            attrs.enum(["x86_64", "aarch64"]),
            default = None,
            doc = "Build this image for a specific target arch without using `buck -c`",
        ),
    } | (
        # @oss-disable
        # @oss-enable {}
    )

def _flavor_name(base: str, rou: str) -> str:
    if rou == "rou-stable":
        return base

    # unfortunate naming convention carried from antlir1
    if rou == "rou-test":
        return base + "-rou-untested"
    return base + "-" + rou

def _rou_flavor_sel(base_flavor: str) -> Select:
    return select({
        antlir2_dep("//antlir/antlir2/os/facebook:" + rou): antlir2_dep("//antlir/antlir2/facebook/flavor/{flavor}:{flavor}".format(
            flavor = _flavor_name(base_flavor, rou),
        ))
        for rou in [
            "rou-preparation",
            "rou-rolling",
            "rou-stable",
            "rou-test",
        ]
    })

def attrs_selected_by_cfg():
    return {
        # only attrs.option because it cannot be set on build appliance layers
        "flavor": attrs.option(
            attrs.dep(providers = [FlavorInfo]),
            default = select({
                antlir2_dep("//antlir/antlir2/os:centos8"): _rou_flavor_sel("centos8"),
                antlir2_dep("//antlir/antlir2/os:centos9"): _rou_flavor_sel("centos9"),
                antlir2_dep("//antlir/antlir2/os:eln"): _rou_flavor_sel("eln"),
                antlir2_dep("//antlir/antlir2/os:none"): antlir2_dep("//antlir/antlir2/flavor:none"),
                antlir2_dep("//antlir/antlir2/os:rhel8"): antlir2_dep("//antlir/antlir2/facebook/flavor/rhel8:rhel8"),
                # TODO: in D49383768 this will be disallowed so that we can
                # guarantee that we'll never end up building a layer without
                # configuring the os
                "DEFAULT": None,
            }),
        ),
        "_rootless": attrs.default_only(attrs.bool(default = select({
            antlir2_dep("//antlir/antlir2/antlir2_rootless:rootless"): True,
            antlir2_dep("//antlir/antlir2/antlir2_rootless:rooted"): False,
            "DEFAULT": False,
        }))),
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

    # If a build appliance is being built, we must remove the OS configuration
    # constraints to avoid circular dependencies.
    if attrs.antlir_internal_build_appliance:
        constraints = remove_os_constraints(refs = refs, constraints = constraints)

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
        constraints = fb_transition(refs, attrs, constraints, overwrite = False)

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
        "package_manager_constraint": antlir2_dep("//antlir/antlir2/os/package_manager:package_manager"),
        "package_manager_dnf": antlir2_dep("//antlir/antlir2/os/package_manager:dnf"),
        "rooted": antlir2_dep("//antlir/antlir2/antlir2_rootless:rooted"),
        "rootless": antlir2_dep("//antlir/antlir2/antlir2_rootless:rootless"),
    } | (
        # @oss-disable
        # @oss-enable {}
    ) | os_transition_refs(),
    attrs = cfg_attrs().keys() + [
        # Build appliances are very low level and cannot depend on a flavor, so
        # they are just not transitioned to an os configuration
        "antlir_internal_build_appliance",
    ],
)

def _remove_os_impl(platform: PlatformInfo, refs: struct) -> PlatformInfo:
    constraints = remove_os_constraints(
        refs = refs,
        constraints = platform.configuration.constraints,
    )
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
        "os_constraint": antlir2_dep("//antlir/antlir2/os:os"),
        "os_family_constraint": antlir2_dep("//antlir/antlir2/os/family:family"),
        "package_manager_constraint": antlir2_dep("//antlir/antlir2/os/package_manager:package_manager"),
        # @oss-disable
    },
)
