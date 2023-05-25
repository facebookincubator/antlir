# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")

def _from_antlir1_flavor(flavor: [str.type, ""]) -> str.type:
    if not types.is_string(flavor):
        flavor = flavor.unaliased_name
    if flavor.startswith("//antlir/facebook/flavor:"):
        flavor = flavor[len("//antlir/facebook/flavor:"):]
    if ":" not in flavor:
        if flavor == "centos8":
            flavor = "//antlir/antlir2/facebook/flavor/centos8:centos8"
        elif flavor == "antlir_test":
            flavor = "//antlir/antlir2/test_images:test-image-flavor"
        elif flavor == "eln":
            flavor = "//antlir/antlir2/facebook/flavor/eln:eln"
        else:
            flavor = "//antlir/antlir2/facebook/flavor:" + flavor

    return flavor

compat = struct(
    from_antlir1_flavor = _from_antlir1_flavor,
)
