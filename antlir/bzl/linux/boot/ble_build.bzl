# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")
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
            template = "//antlir/bzl/linux/boot:loader",
        )

        features.extend([
            feature.install(
                antlir_dep("vm/initrd:{}-initrd".format(kernel.uname)),
                "/initrd-{}.img".format(kernel.uname),
            ),
            feature.install(
                kernel.derived_targets.vmlinuz,
                "/vmlinuz-{}".format(kernel.uname),
            ),
            feature.install(
                ":loader-{}-{}".format(name, kernel.uname),
                "/loader/entries/{}.conf".format(kernel.uname),
            ),
        ])

    image.layer(
        name = name + "__layer",
        parent_layer = parent_layer,
        features = features,
    )

    package.new(
        name = name,
        layer = ":" + name + "__layer",
        format = "vfat",
        loopback_opts = image.opts(
            size_mb = efi_size_mb,
            label = "efi",
        ),
    )
