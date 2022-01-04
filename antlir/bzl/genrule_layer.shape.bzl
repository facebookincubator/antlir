# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load(":container_opts.shape.bzl", "container_opts_t")

genrule_layer_t = shape.shape(
    # IMPORTANT: Be very cautious about adding keys here, specifically
    # rejecting any options that might compromise determinism / hermeticity.
    # Genrule layers effectively run arbitrary code, so we should never
    # allow access to the network, nor read-write access to files outside of
    # the layer.  If you need something from the genrule layer, build it,
    # then reach into it with `image.source`.
    cmd = shape.list(str),
    user = str,
    container_opts = container_opts_t,
    bind_repo_ro = shape.field(bool, default = False),
    boot = shape.field(bool, default = False),
)
