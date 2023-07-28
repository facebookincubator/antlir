# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/bzl:build_defs.bzl", "get_visibility")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl:types.bzl", "types")
load("//metalos/kernel:kernel.shape.bzl", "kernel_t")

_KERNEL_T = types.shape(kernel_t)
_OPT_STR = types.optional(types.str)
_OPT_VISIBILITY = types.optional(types.visibility)

types.lint_noop(_KERNEL_T, _OPT_STR, _OPT_VISIBILITY)

def initrd(
        kernel: _KERNEL_T,
        *,
        name: _OPT_STR = None,
        features = None,
        visibility: _OPT_VISIBILITY = None):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine.
    """

    if not name:
        name = "{}-initrd".format(kernel.uname)
    visibility = get_visibility(visibility)

    # Build an initrd specifically for operating as a VM. This is built on top of the
    # MetalOS initrd and modified to support 9p shared mounts for the repository,
    # kernel modules, and others.

    image.layer(
        name = name + "-layer",
        parent_layer = "//metalos/initrd:initrd",
        visibility = visibility,
        features = [
            feature.ensure_subdirs_exist(
                into_dir = "/usr/lib",
                subdirs_to_create = "modules-load.d",
            ),
            feature.install(
                src = "//antlir/vm/initrd:modules.conf",
                dst = "/usr/lib/modules-load.d/vm.conf",
            ),
            feature.ensure_subdirs_exist(
                into_dir = "/usr/lib",
                subdirs_to_create = "modules",
            ),
            feature.install(
                src = kernel.derived_targets.disk_boot_modules,
                dst = paths.join("/usr/lib/modules", kernel.uname) + "/",
            ),
            systemd.install_dropin("//antlir/vm/initrd:reboot-on-fail.conf", "default.target", use_antlir2 = True),
            systemd.install_dropin("//antlir/vm/initrd:reboot-on-fail.conf", "metalos-init.service", use_antlir2 = True),
            # vm has no network
            systemd.skip_unit("systemd-networkd-wait-online.service", use_antlir2 = True),
        ] + (features or []),
    )

    package.cpio_gz(
        name = name,
        layer = ":" + name + "-layer",
        visibility = visibility,
    )
