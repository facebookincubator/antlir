# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":macro.bzl", "package_macro")

CafLayerPackageInfo = provider(fields = {"subvol_symlink": Artifact})

def _impl(ctx: AnalysisContext) -> list[Provider]:
    li = ctx.attrs.layer[LayerInfo]
    return [
        ctx.attrs.layer[DefaultInfo],
        CafLayerPackageInfo(subvol_symlink = li.subvol_symlink),
    ]

_caf = rule(
    impl = _impl,
    attrs = layer_attrs | {
        "labels": attrs.list(attrs.string(), default = []),
    },
    cfg = package_cfg,
)

caf = package_macro(_caf)
