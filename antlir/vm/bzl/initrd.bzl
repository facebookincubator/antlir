# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "get_visibility")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")
load("//antlir/vm/bzl:install_kernel_modules.bzl", "install_kernel_modules")

DEFAULT_MODULE_LIST = [
    "drivers/block/virtio_blk.ko",
    "drivers/block/loop.ko",
    "drivers/char/hw_random/virtio-rng.ko",
    "drivers/net/net_failover.ko",
    "drivers/net/virtio_net.ko",
    "fs/9p/9p.ko",
    "net/9p/9pnet.ko",
    "net/9p/9pnet_virtio.ko",
    "net/core/failover.ko",
    "drivers/nvme/host/nvme.ko",
    "drivers/nvme/host/nvme-core.ko",
]

def initrd(kernel, module_list = None, visibility = None):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine.

    The init is built "from scratch" with busybox which allows us easier
    customization as well as much faster build time than using dracut.
    """

    name = "{}-initrd".format(kernel.uname)
    module_list = module_list or DEFAULT_MODULE_LIST
    visibility = get_visibility(visibility, name)

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
            where = "/rootdisk/usr/lib/modules/{}".format(kernel.uname),
            type = "9p",
            options = ["ro", "trans=virtio", "version=9p2000.L", "cache=loose", "posixacl"],
        ),
    )
    mount_unit_name = systemd.escape("/rootdisk/usr/lib/modules/{}.mount".format(kernel.uname), path = True)

    # Build an initrd specifically for operating as a VM. This is built on top of the
    # MetalOS initrd and modified to support 9p shared mounts for the repository,
    # kernel modules, and others.
    initrd_vm_features = [
        # The switchroot behavior is different for the vmtest based initrd so
        # lets remove the metalos-switch-root.service and install our own
        feature.remove("/usr/lib/systemd/system/metalos-switch-root.service"),
        feature.remove("/usr/lib/systemd/system/initrd-switch-root.target.requires/metalos-switch-root.service"),
        systemd.install_unit(antlir_dep("vm/initrd:initrd-switch-root.service")),
        systemd.enable_unit("initrd-switch-root.service", target = "initrd-switch-root.target"),
        install_kernel_modules(kernel, module_list),
        # mount kernel modules over 9p in the initrd so they are available
        # immediately in the base os.
        systemd.install_unit(":" + name + "--modules.mount", mount_unit_name),
        systemd.enable_unit(mount_unit_name, target = "initrd-fs.target"),
    ]

    image.layer(
        name = name + "--layer",
        parent_layer = "//metalos/initrd:base",
        # Do not add features directly here, instead add them to
        # initrd_vm_features so they are shared with the debug initrd.
        features = initrd_vm_features,
        visibility = [],
    )

    image.layer(
        name = name + "--layer--debug",
        parent_layer = "//metalos/initrd/debug:debug",
        # Do not add features directly here, instead add them to
        # initrd_vm_features so they are shared with the debug initrd.
        features = initrd_vm_features,
        visibility = [],
    )

    package.new(
        name = name,
        layer = ":" + name + "--layer",
        format = "cpio.gz",
        visibility = visibility,
    )

    package.new(
        name = name + "-debug",
        layer = ":" + name + "--layer--debug",
        format = "cpio.gz",
        visibility = visibility,
    )
