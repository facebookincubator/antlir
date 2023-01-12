# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:layer.bzl", "layer")
load("//antlir/buck2/bzl/feature:genrule.bzl", "genrule")
load("//antlir/bzl:container_opts.bzl", "normalize_container_opts")
load("//antlir/bzl:container_opts.shape.bzl", "container_opts_t")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop(container_opts_t)

def genrule_layer(
        *,
        name: str.type,
        cmd: [str.type],
        user: str.type = "nobody",
        container_opts: [types.shape(container_opts_t), None] = None,
        bind_repo_ro: bool.type = False,
        boot: bool.type = False,
        **kwargs):
    if "features" in kwargs:
        fail("'features' not allowed here")
    container_opts = normalize_container_opts(container_opts)
    layer(
        name = name,
        features = [
            genrule(
                cmd = cmd,
                user = user,
                container_opts = container_opts,
                bind_repo_ro = bind_repo_ro,
                boot = boot,
            ),
        ],
        **kwargs
    )
