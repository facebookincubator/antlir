# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This provides helpers useful for working with flavors. For more
information check out [the flavor docs](/docs/concepts/rpms/overview).
"""

def _get_antlir_linux_flavor():
    return "centos8"

def _get_shortname(flavor):
    shortname = getattr(flavor, "shortname", None)
    return shortname or {
        "centos8": "c8",
        "centos9": "c9",
    }.get(flavor, flavor)

flavor_helpers = struct(
    get_antlir_linux_flavor = _get_antlir_linux_flavor,
    get_shortname = _get_shortname,
)
