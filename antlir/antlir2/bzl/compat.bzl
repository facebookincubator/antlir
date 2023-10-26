# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "is_buck2")
load("//antlir/bzl:types.bzl", "types")

def _from_antlir1_flavor(
        flavor: str | typing.Any,
        *,
        strip_rou: bool = False) -> str | None:
    if not flavor:
        return None
    if not types.is_string(flavor):
        flavor = flavor.unaliased_name

    if is_buck2():
        flavor = flavor.removesuffix("-aarch64")
    elif flavor.endswith("-aarch64"):
        flavor = flavor[:-len("-aarch64")]

    if ":" not in flavor:
        if strip_rou:
            flavor = flavor.split("-", 1)
            flavor = flavor[0]

        if flavor.endswith("-untested") and "-rou-" not in flavor:
            if is_buck2():
                flavor = flavor.removesuffix("-untested")
            else:
                flavor = flavor[:-len("-untested")]

        if flavor == "antlir_test":
            flavor = "//antlir/antlir2/test_images:test-image-flavor"
        else:
            flavor = "//antlir/antlir2/facebook/flavor/{flavor}:{flavor}".format(flavor = flavor)

    return flavor

def _flavor_config_override_to_versionlock_extend(flavor_config_override):
    versionlock_extend = None
    if flavor_config_override and hasattr(flavor_config_override, "rpm_version_set_overrides") and flavor_config_override.rpm_version_set_overrides:
        versionlock_extend = {}
        for nevra in flavor_config_override.rpm_version_set_overrides:
            versionlock_extend[nevra.name] = "{}:{}-{}.{}".format(
                nevra.epoch,
                nevra.version,
                nevra.release,
                nevra.arch,
            )
    return versionlock_extend

compat = struct(
    from_antlir1_flavor = _from_antlir1_flavor,
    flavor_config_override_to_versionlock_extend = _flavor_config_override_to_versionlock_extend,
)
