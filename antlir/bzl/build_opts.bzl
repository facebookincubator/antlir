# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":check_flavor_exists.bzl", "check_flavor_exists")
load(":constants.bzl", "DO_NOT_USE_BUILD_APPLIANCE", "REPO_CFG")
load(":snapshot_install_dir.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")

def _build_opts(
        # The name of the btrfs subvolume to create.
        subvol_name = "volume",
        # Path to a layer target of a build appliance, containing an
        # installed `rpm_repo_snapshot()`, plus an OS image with other
        # image build tools like `btrfs`, `dnf`, `yum`, `tar`, `ln`, ...
        build_appliance = REPO_CFG.build_appliance_default,
        # A "flavor" name, see `bzl/constants.bzl`.
        # Currently used for RPM flavor selection.
        #
        # Future: refer to the OSS "flavor selection" doc once ready.
        flavor = REPO_CFG.flavor_default,
        # The build appliance currently does not set a default package
        # manager -- in non-default settings, this has to be chosen per
        # image, since a BA can support multiple package managers.  In the
        # future, if specifying a non-default installer per image proves
        # onerous when using non-default BAs, we could support a `default`
        # symlink under `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
        rpm_installer = REPO_CFG.rpm_installer_default,
        # List of target or /__antlir__ paths, see `snapshot_install_dir` doc.
        #
        # `None` uses the default determined by looking up `rpm_installer`
        # in `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
        rpm_repo_snapshot = None,
        # List of nevra objects (see antlir/bzl/image_rpm.bzl for definition).
        # If rpm with given name to be installed, the nevra defines its version.
        rpm_version_set_overrides = None):
    if build_appliance == None:
        fail(
            "Must be a target path, or a value from `constants.bzl`",
            "build_appliance",
        )

    if rpm_installer != "yum" and rpm_installer != "dnf":
        fail("Unsupported rpm_installer supplied in build_opts")

    # When building the BA itself, we need this constant to avoid a circular
    # dependency.
    #
    # This feature is exposed a non-`None` magic constant so that callers
    # cannot get confused whether `None` refers to "no BA" or "default BA".
    if build_appliance == DO_NOT_USE_BUILD_APPLIANCE:
        build_appliance = None

    check_flavor_exists(flavor)

    return struct(
        build_appliance = build_appliance,
        flavor = flavor,
        rpm_installer = rpm_installer,
        rpm_repo_snapshot = (
            snapshot_install_dir(rpm_repo_snapshot) if rpm_repo_snapshot else None
        ),
        subvol_name = subvol_name,
        rpm_version_set_overrides = rpm_version_set_overrides,
    )

def normalize_build_opts(build_opts):
    return _build_opts(**(structs.to_dict(build_opts) if build_opts else {}))
