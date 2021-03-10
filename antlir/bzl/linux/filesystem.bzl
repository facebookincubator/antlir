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
        image.ensure_dirs_exist("/usr/bin"),
        image.ensure_dirs_exist("/usr/lib"),
        image.ensure_dirs_exist("/usr/lib64"),
        image.ensure_dirs_exist("/var"),
        image.symlink_dir("/usr/bin", "/bin"),
        image.symlink_dir("/usr/bin", "/sbin"),
        image.symlink_dir("/usr/lib", "/lib"),
        image.symlink_dir("/usr/lib64", "/lib64"),
        image.symlink_dir("/run", "/var/run"),
    ]

filesystem = struct(
    install = _install,
)
