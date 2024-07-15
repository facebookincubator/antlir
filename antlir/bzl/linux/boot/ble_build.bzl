# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/bzl:shape.bzl", "shape")
load(":boot_loader_entry.shape.bzl", "boot_loader_entry_t")

def ble_build(
        name,
        kernels,
        label,
        args,
        parent_layer,
        efi_size_mb: int = 256):
    args = args or []
    args.extend([
        "root=LABEL={}".format(label),
        "console=ttyS0",
        "rootfstype=btrfs",
    ])

    features = []
    for kernel in kernels:
        shape.render_template(
            name = "loader-{}-{}".format(name, kernel.uname),
            instance = boot_loader_entry_t(
                title = "Metal OS - {}".format(kernel.uname),
                kernel = "/vmlinuz-{}".format(kernel.uname),
                initrds = [
                    "/initrd-{}.img".format(kernel.uname),
                ],
                options = args,
            ),
            template = "antlir//antlir/bzl/linux/boot:loader",
        )

        features.extend([
            feature.install(
                src = "//metalos/vm/initrd:vm-{}-initrd".format(kernel.uname),
                dst = "/initrd-{}.img".format(kernel.uname),
            ),
            feature.install(
                src = kernel.vmlinuz,
                dst = "/vmlinuz-{}".format(kernel.uname),
            ),
            feature.install(
                src = ":loader-{}-{}".format(name, kernel.uname),
                dst = "/loader/entries/{}.conf".format(kernel.uname),
            ),
        ])

    image.layer(
        name = name + "__layer",
        parent_layer = parent_layer,
        features = features,
    )

    package.vfat(
        name = name,
        layer = ":" + name + "__layer",
        size_mb = efi_size_mb,
        label = "efi",
    )
