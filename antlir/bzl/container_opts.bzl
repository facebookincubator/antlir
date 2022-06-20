# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load(":container_opts.shape.bzl", "container_opts_t")
load(":snapshot_install_dir.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")

def _new_container_opts_t(
        # List of target or /__antlir__ paths, see `snapshot_install_dir` doc.
        serve_rpm_snapshots = (),
        proxy_server_config = None,
        **kwargs):
    return container_opts_t(
        serve_rpm_snapshots = [
            snapshot_install_dir(s)
            for s in serve_rpm_snapshots
        ],
        proxy_server_config = proxy_server_config,
        **kwargs
    )

def normalize_container_opts(container_opts):
    if not container_opts:
        container_opts = {}
    if types.is_dict(container_opts):
        return _new_container_opts_t(**container_opts)
    return _new_container_opts_t(**structs.to_dict(container_opts))
