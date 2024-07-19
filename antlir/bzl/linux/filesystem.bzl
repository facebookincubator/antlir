# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")

def _install():
    return [
        feature.ensure_dirs_exist(
            dirs = "/dev",
        ),
        feature.ensure_dirs_exist(
            dirs = "/etc",
        ),
        feature.ensure_dirs_exist(
            dirs = "/proc",
        ),
        feature.ensure_dirs_exist(
            dirs = "/run",
        ),
        feature.ensure_dirs_exist(
            dirs = "/sys",
        ),
        feature.ensure_dirs_exist(
            dirs = "/tmp",
        ),
        feature.ensure_dirs_exist(
            dirs = "/usr",
            mode = 0o755,
        ),
        feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "bin",
            mode = 0o555,
        ),
        feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "lib",
            mode = 0o555,
        ),
        feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "lib64",
            mode = 0o555,
        ),
        feature.ensure_subdirs_exist(
            into_dir = "/usr",
            subdirs_to_create = "sbin",
            mode = 0o555,
        ),
        feature.ensure_dirs_exist(
            dirs = "/var",
        ),
        feature.ensure_dir_symlink(
            link = "/bin",
            target = "/usr/bin",
        ),
        feature.ensure_dir_symlink(
            link = "/sbin",
            target = "/usr/sbin",
        ),
        feature.ensure_dir_symlink(
            link = "/lib",
            target = "/usr/lib",
        ),
        feature.ensure_dir_symlink(
            link = "/lib64",
            target = "/usr/lib64",
        ),
        feature.ensure_dir_symlink(
            link = "/var/run",
            target = "/run",
        ),
    ]

filesystem = struct(
    install = _install,
)
