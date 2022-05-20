# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "get_visibility")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")

def initrd(kernel, visibility = None, mount_modules = True):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine.
    """

    name = "{}-initrd".format(kernel.uname)
    visibility = get_visibility(visibility)

    systemd.units.mount_file(
        name = name + "--modules.mount",
        mount = shape.new(
            systemd.units.mount,
            unit = shape.new(
                systemd.units.unit,
                description = "Full set of kernel modules",
                requires = ["systemd-modules-load.service"],
                after = ["systemd-modules-load.service"],
                before = ["initrd-fs.target"],
            ),
            what = "kernel-modules",
            where = "/sysroot/usr/lib/modules/{}".format(kernel.uname),
            type = "9p",
            options = ["ro", "trans=virtio", "version=9p2000.L", "cache=loose", "posixacl"],
        ),
    )
    mount_unit_name = systemd.escape("/sysroot/usr/lib/modules/{}.mount".format(kernel.uname), path = True)

    # Build an initrd specifically for operating as a VM. This is built on top of the
    # MetalOS initrd and modified to support 9p shared mounts for the repository,
    # kernel modules, and others.
    initrd_vm_features = [
        systemd.install_unit(antlir_dep("vm/initrd:initrd-switch-root.service")),
        systemd.enable_unit("initrd-switch-root.service", target = "initrd-switch-root.target"),
        systemd.install_unit(antlir_dep("vm/initrd:sysroot.mount")),
        image.ensure_subdirs_exist("/usr/lib", "modules-load.d"),
        feature.install("//antlir/vm/initrd:modules.conf", "/usr/lib/modules-load.d/vm.conf"),
        image.ensure_subdirs_exist("/usr/lib", paths.join("modules", kernel.uname)),
        feature.install(kernel.derived_targets.disk_boot_modules, paths.join("/usr/lib/modules", kernel.uname)),
    ]

    if mount_modules:
        # mount kernel modules over 9p in the initrd so they are available
        # immediately in the base os.
        initrd_vm_features.extend([
            systemd.install_unit(":" + name + "--modules.mount", mount_unit_name),
            systemd.enable_unit(mount_unit_name, target = "initrd-fs.target"),
        ])

    image.layer(
        name = name + "-layer",
        parent_layer = "//metalos/initrd:initrd-common",
        # Do not add features directly here, instead add them to
        # initrd_vm_features so they are shared with the debug initrd.
        features = initrd_vm_features,
        visibility = visibility,
    )

    package.new(
        name = name,
        layer = ":" + name + "-layer",
        format = "cpio.gz",
        visibility = visibility,
    )
