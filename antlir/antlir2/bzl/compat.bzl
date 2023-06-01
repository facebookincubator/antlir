# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")

def _from_antlir1_flavor(flavor: [str.type, ""]) -> str.type:
    if not types.is_string(flavor):
        flavor = flavor.unaliased_name

    if ":" not in flavor:
        # antlir2 does not have suffixes like -untested or -rou-preparation
        # because it does not need them
        flavor = flavor.split("-", 1)
        flavor = flavor[0]
        if flavor == "antlir_test":
            flavor = "//antlir/antlir2/test_images:test-image-flavor"
        else:
            flavor = "//antlir/antlir2/facebook/flavor/{flavor}:{flavor}".format(flavor = flavor)

    return flavor

compat = struct(
    from_antlir1_flavor = _from_antlir1_flavor,
)
