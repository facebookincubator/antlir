# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")

def _install():
    return [
        antlir2_feature.ensure_dirs_exist(
            dirs = "/dev",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/etc",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/proc",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/run",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/sys",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/tmp",
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/usr",
            mode = 0o755,
        ),
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "bin",
            mode = 0o555,
        ),
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "lib",
            mode = 0o555,
        ),
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "lib64",
            mode = 0o555,
        ),
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "sbin",
            mode = 0o555,
        ),
        antlir2_feature.ensure_dirs_exist(
            dirs = "/var",
        ),
        antlir2_feature.ensure_dir_symlink(
            link = "/bin",
            target = "/usr/bin",
        ),
        antlir2_feature.ensure_dir_symlink(
            link = "/sbin",
            target = "/usr/sbin",
        ),
        antlir2_feature.ensure_dir_symlink(
            link = "/lib",
            target = "/usr/lib",
        ),
        antlir2_feature.ensure_dir_symlink(
            link = "/lib64",
            target = "/usr/lib64",
        ),
        antlir2_feature.ensure_dir_symlink(
            link = "/var/run",
            target = "/run",
        ),
    ]

filesystem = struct(
    install = _install,
)
