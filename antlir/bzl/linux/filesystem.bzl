# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _install():
    return [
        feature.ensure_dirs_exist("/dev"),
        feature.ensure_dirs_exist("/etc"),
        feature.ensure_dirs_exist("/proc"),
        feature.ensure_dirs_exist("/run"),
        feature.ensure_dirs_exist("/sys"),
        feature.ensure_dirs_exist("/tmp"),
        feature.ensure_dirs_exist("/usr", mode = 0o755),
        feature.ensure_subdirs_exist("/usr", "bin", mode = 0o555),
        feature.ensure_subdirs_exist("/usr", "lib", mode = 0o555),
        feature.ensure_subdirs_exist("/usr", "lib64", mode = 0o555),
        feature.ensure_subdirs_exist("/usr", "sbin", mode = 0o555),
        feature.ensure_dirs_exist("/var"),
        feature.ensure_dir_symlink("/usr/bin", "/bin"),
        feature.ensure_dir_symlink("/usr/sbin", "/sbin"),
        feature.ensure_dir_symlink("/usr/lib", "/lib"),
        feature.ensure_dir_symlink("/usr/lib64", "/lib64"),
        feature.ensure_dir_symlink("/run", "/var/run"),
    ]

filesystem = struct(
    install = _install,
)
