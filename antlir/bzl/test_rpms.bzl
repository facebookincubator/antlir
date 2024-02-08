# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This provides helpers useful for installing rpms on layers with flavor `antlir_test`. For more
information check out [the flavor docs](/docs/concepts/flavors/inheritance-in-parent-layers).
"""

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _add(rpmlist):
    """
    This wraps `rpms_install` but includes dummy ones for the remaining flavors in `REPO_CFG.flavor_available`
    to skip the coverage checks in layers with inherited flavors. For more information see [the documentation](/docs/concepts/flavors/inheritance-in-parent-layers)

    Arguments
    - `rpmslist`: The list of test rpms to wrap in `rpms_install` with flavor `antlir_test`.
    """
    return [
        feature.rpms_install(rpmlist),
    ]

def _remove(rpmlist):
    """
    Similar to `_add` but for `rpms_remove_if_exists`.

    Arguments
    - `rpmslist`: The list of test rpms to wrap in `rpms_install` with flavor `antlir_test`.
    """
    return [
        feature.rpms_remove_if_exists(rpmlist, flavors = ["antlir_test"]),
    ] + [
        feature.rpms_remove_if_exists([], flavors = REPO_CFG.flavor_available),
    ]

test_rpms = struct(
    add = _add,
    remove = _remove,
)
