# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")

def _install():
    return [
        image.ensure_dirs_exist("/dev"),
        image.ensure_dirs_exist("/etc"),
        image.ensure_dirs_exist("/proc"),
        image.ensure_dirs_exist("/run"),
        image.ensure_dirs_exist("/sys"),
        image.ensure_dirs_exist("/tmp"),
        image.ensure_dirs_exist("/usr", mode = 0o755),
        image.ensure_subdirs_exist("/usr", "bin", mode = 0o555),
        image.ensure_subdirs_exist("/usr", "lib", mode = 0o555),
        image.ensure_subdirs_exist("/usr", "lib64", mode = 0o555),
        image.ensure_subdirs_exist("/usr", "sbin", mode = 0o555),
        image.ensure_dirs_exist("/var"),
        image.ensure_dir_symlink("/usr/bin", "/bin"),
        image.ensure_dir_symlink("/usr/sbin", "/sbin"),
        image.ensure_dir_symlink("/usr/lib", "/lib"),
        image.ensure_dir_symlink("/usr/lib64", "/lib64"),
        image.ensure_dir_symlink("/run", "/var/run"),
    ]

filesystem = struct(
    install = _install,
)
