# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "kernel_get")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")

loader_entry_t = shape.shape(
    title = str,
    kernel = str,
    initrds = shape.list(str),
    options = shape.list(str),
)

def _build(
        name,
        # A list of kernel_t (from //antlir/vm/bzl:kernel.bzl) instances.
        # Each referenced kernel will be inserted into this boot setup
        # with a unique BLS.
        kernels,
        # The label name of the rootfs device.
        label = "/",
        # A list of additional name=value arguments to pass on the
        # kernel cmd line.
        args = None):
    args = args or []
    args.extend([
        "root=LABEL={}".format(label),
        "rw",
        "console=ttyS0",
        "rootfstype=btrfs",
    ])

    features = []
    for kernel in kernels:
        shape.render_template(
            name = "loader-{}".format(kernel.uname),
            instance = shape.new(
                loader_entry_t,
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
                "{}:{}-initrd".format(
                    kernel_get.base_target,
                    kernel.uname,
                ),
                "/initrd-{}.img".format(kernel.uname),
            ),
            feature.install(
                kernel.artifacts.vmlinuz,
                "/vmlinuz-{}".format(kernel.uname),
            ),
            feature.install(
                ":loader-{}".format(kernel.uname),
                "/loader/entries/{}.conf".format(kernel.uname),
            ),
        ])

    image.layer(
        name = name + "__layer",
        parent_layer = "//antlir/bzl/linux/boot/systemd:base",
        features = features,
    )

    package.new(
        name = name,
        layer = ":" + name + "__layer",
        format = "vfat",
        loopback_opts = image.opts(
            size_mb = 256,
            label = "efi",
        ),
    )

boot = struct(
    build = _build,
)
