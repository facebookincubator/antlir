# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":flavor.shape.bzl", "flavor_t")
load(":structs.bzl", "structs")

_flavor_t_type = type(flavor_t(name = ""))

def _flavor_t_verify(flavor):
    if type(flavor) == _flavor_t_type:
        if flavor.name == "":
            fail("Empty flavor name")
    return flavor

def flavor_to_struct(flavor):
    """Given a flavor name or flavor struct, return a flavor struct."""
    if type(flavor) == type(""):
        flavor = flavor_t(name = flavor)
    elif type(flavor) == type({}):
        flavor = flavor_t(**flavor)
    return _flavor_t_verify(flavor)

def flavor_to_dict(flavor):
    """Given a flavor name or flavor struct, return a flavor dict."""
    if not flavor:
        return flavor
    flavor = structs.to_dict(flavor_to_struct(flavor))

    # XXX: The conversion above adds a magical "shape" key to the
    # dictionary. We want to delete it, but we can't actually reference
    # it by name (any attempts to reference to it always fails to find
    # it). So instead we filter by keys we know.
    known_keys = ["name"]
    flavor = {k: v for k, v in flavor.items() if k in known_keys}
    return flavor

def flavors_to_structs(flavors):
    """Given a list of flavor names, return a list of flavor structs."""
    if not flavors:
        return []
    return [flavor_to_struct(i) for i in flavors]

def flavors_to_dicts(flavors):
    """Given a list of flavor names, return a list of flavor dicts."""
    if not flavors:
        return []
    return [flavor_to_dict(i) for i in flavors]

def flavor_to_name(flavor):
    if not flavor:
        return flavor
    return flavor_to_struct(flavor).name

def flavors_to_names(flavors):
    if not flavors:
        return []
    return [flavor_to_name(i) for i in flavors]
