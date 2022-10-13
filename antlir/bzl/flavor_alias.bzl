# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _get_flavor_alias():
    val = native.read_config("antlir", "flavor-alias")
    if val == None:
        return None
    if len(val.split("=")) != 2:
        fail(
            "antlir.flavor-alias is not a <flavor1>=<flavor2> pair: " +
            "{}".format(val),
        )
    return val.split("=")

def aliased_flavor_sources():
    alias = _get_flavor_alias()
    return [alias[1]] if alias != None else []

def aliased_flavor_targets():
    alias = _get_flavor_alias()
    return [alias[0]] if alias != None else []

def _fail_if_flavor_aliasing_disabled():
    if native.read_config("antlir", "fail-on-flavor-aliasing"):
        #
        # This is a testing option. Certain code paths must not use
        # flavor aliasing, and there for those code paths explicitly
        # disable it, which means this code should never be called.
        # See test_flavor_aliasing_disabled() for usage.
        #
        fail(
            "Antlir flavor aliases are disabled, and this it not a " +
            "required flavor aliasing call.",
        )

def alias_flavor(flavor, required = False):
    if not required:
        _fail_if_flavor_aliasing_disabled()
    if not flavor:
        return flavor
    alias = _get_flavor_alias()
    if alias == None:
        return flavor
    return flavor if flavor != alias[0] else alias[1]

def alias_flavors(flavors, required = False):
    if not flavors:
        return flavors

    # flavor aliasing can generate dups in a list, so filter them out.
    rv = {}
    for i in flavors:
        i = alias_flavor(i, required)
        rv[i] = 1
    return rv.keys()
