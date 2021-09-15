# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load(":shape.bzl", "shape")
load(":snapshot_install_dir.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")

# Forward container runtime configuration to the Python implementation.
# This currently maps to `NspawnPluginArgs`.
#
# Prefer to keep this default-initializable to avoid having to update a
# bunch of tests and other Python callsites.
container_opts_t = shape.shape(
    shadow_proxied_binaries = shape.field(bool, default = False),
    serve_rpm_snapshots = shape.list(shape.path(), default = []),
    # See `--shadow-path` in `args.py`.
    shadow_paths = shape.list(
        shape.tuple(shape.path(), shape.path()),
        default = [],
    ),
    # Do not use this, it is only exposed so that Antlir can populate the
    # repodata caches for the RPM snapshots.
    internal_only_unprotect_antlir_dir = shape.field(bool, default = False),
    # This is exposed here only because we need some way to enable this FB-
    # centric feature in FB container image tests.  A future refactor should
    # take this away and put it into a FB-internal overlay.
    internal_only_logs_tmpfs = shape.field(bool, default = False),
    # mknod really should not be required in a container. It is only included
    # here for an internal Antlir test that needs to cover the interaction with
    # `mknod` and btrfs sendstreams
    internal_only_allow_mknod = shape.field(bool, default = False),
)

def _new_container_opts_t(
        # List of target or /__antlir__ paths, see `snapshot_install_dir` doc.
        serve_rpm_snapshots = (),
        **kwargs):
    return shape.new(
        container_opts_t,
        serve_rpm_snapshots = [
            snapshot_install_dir(s)
            for s in serve_rpm_snapshots
        ],
        **kwargs
    )

def normalize_container_opts(container_opts):
    if not container_opts:
        container_opts = {}
    if types.is_dict(container_opts):
        return _new_container_opts_t(**container_opts)
    return _new_container_opts_t(**structs.to_dict(container_opts))
