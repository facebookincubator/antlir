# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This provides helpers useful for working with flavors. For more
information check out [the flavor docs](/docs/concepts/rpms/overview).
"""

load(":constants.bzl", "REPO_CFG")
load(":flavor_impl.bzl", "get_unaliased_flavor")

# TODO(vmagro): delete this
def _get_flavor_default():
    return "centos8"

def _get_antlir_linux_flavor():
    return REPO_CFG.antlir_linux_flavor

def _get_shortname(flavor):
    # Flavor shortnames are commonly used in target and fbpkg names,
    # where we generally don't want flavor aliasing to be used.
    flavor = get_unaliased_flavor(flavor)
    return REPO_CFG.flavor_to_config[flavor.name].shortname

flavor_helpers = struct(
    get_flavor_default = _get_flavor_default,
    get_antlir_linux_flavor = _get_antlir_linux_flavor,
    get_shortname = _get_shortname,
)
