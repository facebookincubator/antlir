# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

# Forward container runtime configuration to the Python implementation.
# This currently maps to `NspawnPluginArgs`.
#
# Prefer to keep this default-initializable to avoid having to update a
# bunch of tests and other Python callsites.
container_opts_t = shape.shape(
    # Future: move `boot` in here, too.
    boot_await_dbus = shape.field(bool, default = True),
    shadow_proxied_binaries = shape.field(bool, default = False),
    serve_rpm_snapshots = shape.field(shape.list(shape.path), default = []),
    # See `--shadow-path` in `args.py`.
    shadow_paths = shape.field(
        shape.list(
            shape.tuple(shape.path, shape.path),
        ),
        default = [],
    ),
    # Setting this to `True` corresponds to `--attach-antlir-dir=explicit_on`,
    # see `buck run :YOURLAYER=container -- --help` for the docs.
    attach_antlir_dir = shape.field(bool, default = False),

    # Run proxy
    run_proxy_server = shape.field(bool, default = False),
    # @oss-disable: fbpkg_db_path = shape.field(shape.path, optional = True), 

    # Do not use this, it is only exposed so that certain interal
    # unittests that need to create devices can run.
    internal_only_allow_mknod = shape.field(bool, default = False),
    # Do not use this, it is only exposed so that Antlir can populate the
    # repodata caches for the RPM snapshots.
    internal_only_unprotect_antlir_dir = shape.field(bool, default = False),
    # This is exposed here only because we need some way to enable this FB-
    # centric feature in FB container image tests.  A future refactor should
    # take this away and put it into a FB-internal overlay.
    internal_only_logs_tmpfs = shape.field(bool, default = False),
    # This is exposed here to allow certain internal test cases to always
    # bind mount the repo root into the test case regardless of the build
    # mode.
    internal_only_bind_repo_ro = shape.field(bool, default = False),
    internal_only_bind_artifacts_dir_rw = shape.field(bool, default = False),
)
