# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")

DEFAULT_APPLETS = sets.make([
    "basename",
    "blkid",
    "blockdev",
    "cat",
    "clear",
    "cp",
    "date",
    "dd",
    "df",
    "dmesg",
    "du",
    "echo",
    "egrep",
    "env",
    "false",
    "file",
    "find",
    "free",
    "fstrim",
    "fsync",
    "grep",
    "groups",
    "hdparm",
    "head",
    "hexdump",
    "hostname",
    "hwclock",
    "id",
    "insmod",
    "ip",
    "kill",
    "less",
    "ln",
    "ls",
    "lsmod",
    "lspci",
    "lsusb",
    "mkdir",
    "mknod",
    "mktemp",
    "modinfo",
    "modprobe",
    "more",
    "mount",
    "mv",
    "nc",
    "nslookup",
    "partprobe",
    "pgrep",
    "ping",
    "ping6",
    "pkill",
    "ps",
    "pstree",
    "readlink",
    "realpath",
    "reboot",
    "reset",
    "rm",
    "rmdir",
    "rmmod",
    "sed",
    "sh",
    "sha256sum",
    "sleep",
    "sort",
    "strings",
    "su",
    "sync",
    "tail",
    "tar",
    "tee",
    "tftp",
    "time",
    "top",
    "touch",
    "tr",
    "traceroute",
    "traceroute6",
    "true",
    "truncate",
    "umount",
    "uname",
    "uniq",
    "users",
    "vi",
    "wc",
    "wget",
    "which",
    "xargs",
    "xxd",
    "yes",
])

def _install(src, applets = None, install_dir = "/usr/bin", src_path = "/usr/sbin/busybox"):
    """
    Generate features to install a statically linked `busybox` binary
    from the supplied `src` layer into an `install_dir` (default `/usr/bin`)
    and configure a set of applets for it.

    The `src` layer must have the `busybox` binary installed at the path `/busybox`.
    """
    applets = sets.to_list(applets or DEFAULT_APPLETS)
    return [
        antlir2_feature.clone(
            src_layer = src,
            src_path = src_path,
            dst_path = paths.join(install_dir, "busybox"),
        ),
    ] + [
        antlir2_feature.ensure_file_symlink(
            link = paths.join(install_dir, applet),
            target = paths.join(install_dir, "busybox"),
        )
        for applet in applets
    ]

busybox = struct(
    install = _install,
)
