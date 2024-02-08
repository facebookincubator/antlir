# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":constants.bzl", "REPO_CFG")
load(":flavor.shape.bzl", "flavor_t")
load(":target_helpers.bzl", "normalize_target")
load(":types.bzl", "types")

_flavor_t_type = type(flavor_t(name = "", unaliased_name = ""))

def _flavor_t_verify(flavor):
    if type(flavor) != _flavor_t_type:
        fail("Not a flavor_t: {}: {}".format(type(flavor), flavor))
    if not flavor.name:
        fail("Missing flavor name")
    if not flavor.unaliased_name:
        fail("Missing unaliased flavor name")
    return flavor

def flavor_to_struct(flavor):
    """Given a flavor name (or struct), return a flavor struct."""
    if not flavor:
        return flavor

    if types.is_string(flavor):
        # TODO(T139523690) flavors should all be targets
        if ":" in flavor:
            _, flavor = flavor.rsplit(":")
        flavor = flavor_t(
            name = flavor,
            unaliased_name = flavor,
        )

    return _flavor_t_verify(flavor)

def flavors_to_structs(flavors):
    """Given a list of flavor names (or structs), return a list of
    flavor structs."""
    if not flavors:
        return []

    # flavor aliasing can generate dups in a list, so filter them out.
    rv = {}
    for i in flavors:
        j = flavor_to_struct(i)
        rv[(j.name, j.unaliased_name)] = 1
    return [flavor_t(name = i, unaliased_name = j) for i, j in rv.keys()]

def flavor_to_name(flavor):
    if not flavor:
        return flavor
    return flavor_to_struct(flavor).name

def flavors_to_names(flavors):
    if not flavors:
        return []

    # Go through flavors_to_structs() for de-dup.
    return [i.name for i in flavors_to_structs(flavors)]

def get_unaliased_flavor(flavor):
    if not flavor or type(flavor) != type(""):
        fail("Invalid flavor: %s" % flavor)
    return _flavor_t_verify(flavor_t(
        name = flavor,
        unaliased_name = flavor,
    ))

def get_unaliased_flavors(flavors):
    return [get_unaliased_flavor(i) for i in flavors]

def is_unaliased_flavor(flavor):
    if flavor == None:
        return False
    return flavor.name == flavor.unaliased_name

def get_flavor_aliased_layer(layer, flavor):
    """If flavor aliasing is in effect, we should remap BA references."""
    if layer == None or is_unaliased_flavor(flavor):
        return layer
    build_appliances = {
        config.build_appliance: flavor
        for flavor, config in REPO_CFG.flavor_to_config.items()
    }
    nlayer = normalize_target(layer)
    if nlayer not in build_appliances:
        return layer
    flavor = build_appliances[nlayer]
    return REPO_CFG.flavor_to_config[flavor].build_appliance
