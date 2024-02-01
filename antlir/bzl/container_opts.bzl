# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:shape.bzl", "shape")
load(":container_opts.shape.bzl", "container_opts_t")
load(":structs.bzl", "structs")

def _new_container_opts_t(
        proxy_server_config = None,
        **kwargs):
    return container_opts_t(
        proxy_server_config = proxy_server_config,
        **kwargs
    )

def normalize_container_opts(container_opts):
    if not container_opts:
        container_opts = {}
    if types.is_dict(container_opts):
        return _new_container_opts_t(**container_opts)
    if shape.is_instance(container_opts, container_opts_t):
        return _new_container_opts_t(**shape.as_serializable_dict(container_opts))
    return _new_container_opts_t(**structs.to_dict(container_opts))
