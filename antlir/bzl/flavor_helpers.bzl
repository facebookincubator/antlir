# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A flavor is a string identifier that controls build configurations.
Options contained in a flavor can include `build_appliance` as well
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
load(":constants.bzl", "REPO_CFG", "new_flavor_config")
load(":structs.bzl", "structs")

def _get_flavor_config(flavor, flavor_config_override):
    '''
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
    '''
    check_flavor_exists(flavor)

    flavor_config = structs.to_dict(REPO_CFG.flavor_to_config[flavor])
    overrides = structs.to_dict(flavor_config_override) if flavor_config_override else {}

    # This override is forbidden because vset paths are currently consumed
    # in `image/feature/new.bzl`, where per-layer overrides are NOT available.
    if "version_set_path" in overrides:
        fail("Cannot override `version_set_path`", "flavor_config_override")
    flavor_config.update(overrides)

    return new_flavor_config(**flavor_config)

def _get_build_appliance(flavor):
    return REPO_CFG.flavor_to_config[flavor].build_appliance

def _get_rpm_installer(flavor):
    return REPO_CFG.flavor_to_config[flavor].rpm_installer

def _get_rpm_installers_supported():
    rpm_installers = {}
    for _, config in REPO_CFG.flavor_to_config.items():
        if config.rpm_installer:
            rpm_installers[config.rpm_installer] = 1
    return rpm_installers.keys()

flavor_helpers = struct(
    default_flavor_build_appliance = _get_build_appliance(REPO_CFG.flavor_default),
    get_build_appliance = _get_build_appliance,
    get_flavor_config = _get_flavor_config,
    get_rpm_installer = _get_rpm_installer,
    get_rpm_installers_supported = _get_rpm_installers_supported,
)
