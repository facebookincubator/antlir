# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A flavor is a string identifier that controls build configurations.
Optinos contained in a flavor can include `build_appliance` as well
as `rpm_installer`.

This allows us to specify compability between
different images. For example, we can make sure that `centos7` images
do not depend on `centos8` images, which is a breaking version.

It also allows to reuse common build opts throughout the codebase
with less duplication.

Flavors are strings instead of functions because a flavor must have
a stable identity as the source tree evolves. We must be able to
compare flavors between old revs and new. The container runtime must
also be able to rely on stable flavor IDs. Flavors names must
follow two critical rules:
    - Never change a flavor name
    - Never reuse a flavor name

## Using Flavors

To create a flavor add a mapping to `antlir/bzl/oss_shim_impl.bzl`

```
shim = struct(
    do_not_use_repo_cfg = {
        "flavor_to_config": {
            "your_flavor_here": {
                "build_appliance": "//path/to/your/build/appliance",
                "rpm_installer": "your_rpm_installer",
            },
        },
    },

)
```

Then, you can pass the flavor to images. You can also override the
default value in the flavor with custom ones.

```
image.layer(
    flavor = "your_flavor_here",
    flavor_config_overrides = image.opts(
        build_appliance = "//your/override/build/appliance",
        ...
    )
)
```

## In progress

The flavor of an image is written to the `/.meta`
directory of the image. This allows you to check the compatibility
of sendstreams, as a sendstream could have been built on
an older revision with a different build appliance than what
is in the repo currently.
"""

load(":check_flavor_exists.bzl", "check_flavor_exists")
load(":constants.bzl", "DO_NOT_USE_BUILD_APPLIANCE", "REPO_CFG")
load(":snapshot_install_dir.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")

def _validate_flavor_config(
        build_appliance,
        rpm_installer,
        rpm_repo_snapshot = None,
        rpm_version_set_overrides = None):
    """
    Arguments

    - `build_appliance`: Path to a layer target of a build appliance,
    containing an installed `rpm_repo_snapshot()`, plus an OS image
    with other image build tools like `btrfs`, `dnf`, `yum`, `tar`, `ln`, ...
    - `rpm_installer`: The build appliance currently does not set
    a default package manager -- in non-default settings, this
    has to be chosen per image, since a BA can support multiple
    package managers.  In the future, if specifying a non-default
    installer per image proves onerous when using non-default BAs, we
    could support a `default` symlink under `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
    - `rpm_repo_snapshot`: List of target or `/__antlir__` paths,
    see `snapshot_install_dir` doc. `None` uses the default determined
    by looking up `rpm_installer` in `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
    - `rpm_version_set_overrides`: List of `nevra` objects
    (see antlir/bzl/image_rpm.bzl for definition). If rpm with given name to
    be installed, the `nevra` defines its version.
    """
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

    return struct(
        build_appliance = build_appliance,
        rpm_installer = rpm_installer,
        rpm_repo_snapshot = (
            snapshot_install_dir(rpm_repo_snapshot) if rpm_repo_snapshot else None
        ),
        rpm_version_set_overrides = rpm_version_set_overrides,
    )

def _get_flavor_config(flavor, flavor_config_override):
    '''
    Arguments
    - `flavor`: The name of the flavor to fetch the config.
    - `flavor_config_override`: An opts that contains any overrides for
    the default config of a flavor that will be applied.

    Example usage:
    ```
    load("//antlir/bzl:flavors.bzl", flavor_helpers = "flavor")

    flavor_config = flavor_helpers.get_flavor_config(flavor, flavor_config_override)
    build_appliance = flavor_config["build_appliance"]
    ```
    '''
    check_flavor_exists(flavor)

    flavor_config = dict(REPO_CFG.flavor_to_config[flavor])

    override_dict = structs.to_dict(flavor_config_override) if flavor_config_override else {}
    flavor_config.update(override_dict)

    return _validate_flavor_config(**flavor_config)

flavor = struct(
    get_flavor_config = _get_flavor_config,
)
