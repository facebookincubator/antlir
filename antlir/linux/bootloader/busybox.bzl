# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")

APPLETS = [
    "basename",
    "cat",
    "clear",
    "cp",
    "echo",
    "file",
    "groups",
    "hostname",
    "id",
    "ip",
    # "less" - intentionally excluded to not have messed up color output from
    # systemctl, since busybox's `less` does not support ansi colors
    "ln",
    "ls",
    "lsmod",
    "mkdir",
    "mktemp",
    "modprobe",
    "mount",
    "ping",
    "rm",
    "rmmod",
    "sh",
    "su",
    "true",
    "umount",
    "uname",
]

def clone_busybox(src):
    return [
        image.ensure_dirs_exist("/usr/sbin"),
        image.ensure_dirs_exist("/usr/bin"),
        image.clone(src, "/usr/sbin/busybox", "/usr/bin/busybox"),
    ] + [
        image.symlink_file(
            "/usr/bin/busybox",
            paths.join("/usr/bin", applet),
        )
        for applet in APPLETS
    ]
