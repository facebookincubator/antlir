# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This provides helpers useful for working with flavors. For more
information check out [the flavor docs](/docs/concepts/rpms/overview).
"""

load(":check_flavor_exists.bzl", "check_flavor_exists")
load(":constants.bzl", "REPO_CFG", "new_flavor_config")
load(":flavor_alias.bzl", "alias_flavor")
load(":flavor_impl.bzl", "flavor_to_struct")
load(":shape.bzl", "shape")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")

def _get_flavor_config(flavor, flavor_config_override):
    """
    Arguments
    - `flavor`: The name of the flavor to fetch the config.
    - `flavor_config_override`: An opts that contains any overrides for
    the default config of a flavor that will be applied.

    Example usage:
    ```
    load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")

    flavor_config = flavor_helpers.get_flavor_config(flavor, flavor_config_override)
    build_appliance = flavor_config["build_appliance"]
    ```
    """
    if not flavor and flavor_config_override:
        fail("Please specify the flavor when overriding the flavor config")

    flavor = flavor_to_struct(flavor)
    check_flavor_exists(flavor)

    flavor_config = shape.as_dict_shallow(REPO_CFG.flavor_to_config[flavor.name])
    overrides = structs.to_dict(flavor_config_override) if flavor_config_override else {}

    # This override is forbidden because vset paths are currently consumed
    # in `image/feature/new.bzl`, where per-layer overrides are NOT available.
    if "version_set_path" in overrides:
        fail("Cannot override `version_set_path`", "flavor_config_override")

    if "rpm_installer" in overrides and not "rpm_repo_snapshot" in overrides:
        fail(
            "Please override the `rpm_repo_snapshot` as well to make sure it " +
            "matches `rpm_installer`. Set it to `None` to use the default snapshot.",
        )
    flavor_config.update(overrides)

    return new_flavor_config(**flavor_config)

def _get_flavor_default():
    #
    # Technically we don't need to call alias_flavor() here (since it's
    # already been invoked for this REPO_CFG variable), but we do it
    # anyway to support `fail-on-flavor-aliasing` testing. Basically,
    # alias_flavor() will fail() if flavor aliasing is disabled and we
    # try to return an aliased flavor.
    #
    return alias_flavor(REPO_CFG.flavor_default)

def _get_antlir_linux_flavor():
    # See the comment above in _get_flavor_default().
    return alias_flavor(REPO_CFG.antlir_linux_flavor)

def _get_build_appliance(flavor = None):
    """
    Arguments
    - `flavor`: The flavor of the build appliance to return.
    """
    flavor = flavor_to_struct(flavor or _get_flavor_default())
    return REPO_CFG.flavor_to_config[flavor.name].build_appliance

def _get_rpm_installer(flavor = None):
    """
    Arguments
    - `flavor`: The flavor of the rpm installer to return.
    """
    flavor = flavor_to_struct(flavor or _get_flavor_default())
    return REPO_CFG.flavor_to_config[flavor.name].rpm_installer

def _get_rpm_installers_supported():
    """
    Returns all possible rpm installers in `REPO_CFG.flavor_to_config` deduplicated.
    """
    rpm_installers = {}
    for _, config in REPO_CFG.flavor_to_config.items():
        if config.rpm_installer:
            rpm_installers[config.rpm_installer] = 1
    return rpm_installers.keys()

def _get_flavor_from_build_appliance(build_appliance):
    build_appliance = normalize_target(build_appliance)
    return REPO_CFG.ba_to_flavor[build_appliance]

def _maybe_get_tgt_flavor(tgt):
    tgt = normalize_target(tgt)
    return REPO_CFG.ba_to_flavor.get(
        tgt,
        REPO_CFG.buck1_tgts_to_flavors.get(tgt, None),
    )

flavor_helpers = struct(
    get_build_appliance = _get_build_appliance,
    get_flavor_from_build_appliance = _get_flavor_from_build_appliance,
    get_flavor_default = _get_flavor_default,
    get_antlir_linux_flavor = _get_antlir_linux_flavor,
    get_flavor_config = _get_flavor_config,
    get_rpm_installer = _get_rpm_installer,
    get_rpm_installers_supported = _get_rpm_installers_supported,
    maybe_get_tgt_flavor = _maybe_get_tgt_flavor,
)
