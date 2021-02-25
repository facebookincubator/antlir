# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:collections.bzl", "collections")
load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:systemd.bzl", SYSTEMD_PROVIDER_ROOT = "PROVIDER_ROOT")
load("//antlir/bzl/foreign/extractor:extract.bzl", "extract")

TARGETS = [
    "basic.target",
    "final.target",
    "initrd-fs.target",
    "initrd-root-device.target",
    "initrd-switch-root.target",
    "initrd.target",
    "local-fs-pre.target",
    "local-fs.target",
    "network.target",
    "network-online.target",
    "poweroff.target",
    "shutdown.target",
    "sysinit.target",
    "sysinit.target.wants",
    "umount.target",
]

UNITS = [
    "initrd-cleanup.service",
    "initrd-switch-root.service",
    "systemd-journald-dev-log.socket",
    "systemd-journald.service",
    "systemd-journald.socket",
    "systemd-modules-load.service",
    "systemd-networkd.service",
    "systemd-networkd-wait-online.service",
    "systemd-poweroff.service",
    "systemd-sysusers.service",
    "systemd-tmpfiles-setup-dev.service",
    "systemd-tmpfiles-setup-dev.service",
    "systemd-tmpfiles-setup.service",
    "systemd-udev-trigger.service",
    "systemd-udevd-control.socket",
    "systemd-udevd-kernel.socket",
    "systemd-udevd.service",
]

BINARIES = [
    "/usr/bin/journalctl",
    "/usr/bin/networkctl",
    "/usr/bin/systemctl",
    "/usr/bin/systemd-sysusers",
    "/usr/bin/systemd-tmpfiles",
    "/usr/bin/udevadm",
    "/usr/lib/systemd/systemd",
    "/usr/lib/systemd/systemd-journald",
    "/usr/lib/systemd/systemd-modules-load",
    "/usr/lib/systemd/systemd-networkd",
    "/usr/lib/systemd/systemd-shutdown",
    "/usr/lib/systemd/systemd-sysctl",
    "/usr/lib/systemd/systemd-udevd",
    # for (system) user lookups
    # this is not a part of systemd, so should not _really_ be here, but due to
    # current extractor limitations has to be included in the same image feature
    "/usr/lib64/libnss_files.so.2",
]

# Configs to take unmodified from upstream systemd
CONFIG_FILES = [
    "/usr/lib/tmpfiles.d/systemd.conf",
    "/usr/lib/tmpfiles.d/tmp.conf",
    "/usr/lib/udev/rules.d/99-systemd.rules",
    "/usr/lib/systemd/network/99-default.link",
    "/usr/lib/sysusers.d/basic.conf",
    "/usr/lib/sysusers.d/systemd.conf",
]

def clone_systemd(src):
    binaries = extract.extract(
        binaries = BINARIES,
        dest = "/",
        source = src,
    )

    units = [
        image.ensure_dirs_exist(SYSTEMD_PROVIDER_ROOT),
    ] + [
        image.clone(
            src,
            paths.join(SYSTEMD_PROVIDER_ROOT, unit),
            paths.join(SYSTEMD_PROVIDER_ROOT, unit),
        )
        for unit in UNITS + TARGETS
    ]

    dirs = [paths.dirname(cfg) for cfg in CONFIG_FILES]
    dirs = collections.uniq(dirs)

    configs = [
        image.ensure_dirs_exist(d)
        for d in dirs
    ] + [
        image.clone(
            src,
            cfg,
            cfg,
        )
        for cfg in CONFIG_FILES
    ]

    return [
        binaries,
        units,
        configs,
    ]
