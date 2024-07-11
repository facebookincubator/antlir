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

        flavor = "antlir//antlir/antlir2/facebook/flavor/{flavor}:{flavor}".format(flavor = flavor)

    return flavor

compat = struct(
    from_antlir1_flavor = _from_antlir1_flavor,
)
