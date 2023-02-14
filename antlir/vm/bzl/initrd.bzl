# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:build_defs.bzl", "get_visibility")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")
load("//metalos/kernel:kernel.shape.bzl", "kernel_t")

types.lint_noop(kernel_t)

def initrd(
        kernel: types.shape(kernel_t),
        *,
        name: types.optional(types.str) = None,
        features = None,
        visibility: types.optional(types.visibility) = None):
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

    if features:
        features = flatten.antlir_features(features)

    image.layer(
        name = name + "-layer",
        parent_layer = "//metalos/initrd:initrd",
        visibility = visibility,
        features = [
            feature.ensure_subdirs_exist("/usr/lib", "modules-load.d"),
            feature.install("//antlir/vm/initrd:modules.conf", "/usr/lib/modules-load.d/vm.conf"),
            feature.ensure_subdirs_exist("/usr/lib", paths.join("modules", kernel.uname)),
            feature.install(kernel.derived_targets.disk_boot_modules, paths.join("/usr/lib/modules", kernel.uname)),
            systemd.install_dropin("//antlir/vm/initrd:reboot-on-fail.conf", "default.target"),
            systemd.install_dropin("//antlir/vm/initrd:reboot-on-fail.conf", "metalos-init.service"),
            # vm has no network
            systemd.skip_unit("systemd-networkd-wait-online.service"),
        ] + (features or []),
    )

    package.new(
        name = name,
        layer = ":" + name + "-layer",
        format = "cpio.gz",
        visibility = visibility,
    )
