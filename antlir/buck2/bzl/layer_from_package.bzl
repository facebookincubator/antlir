# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:flavor.bzl", "FlavorInfo")
load("//antlir/buck2/bzl:layer_info.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    return [
        LayerInfo(
            default_mountpoint = ctx.attrs.default_mountpoint,
            features = [],
            flavor = ctx.attrs.flavor,
        ),
        DefaultInfo(),
    ]

layer_from_package = rule(
    impl = _impl,
    attrs = {
        "default_mountpoint": attrs.option(attrs.string()),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo])),
        "format": attrs.enum(["sendstream", "sendstream.v2"]),
        "source": attrs.source(),
    },
)
