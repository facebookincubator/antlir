# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/linux/boot:ble_build.bzl", "ble_build")

def _systemd_build(
        name,
        # A list of kernel_t (from //antlir/bzl:kernel_shim.bzl) instances.
        # Each referenced kernel will be inserted into this boot setup
        # with a unique BLS.
        kernels,
        # The label name of the rootfs device.
        label = "/",
        # A list of additional name=value arguments to pass on the
        # kernel cmd line.
        args = None,
        efi_size_mb: int = 256):
    ble_build(
        name = name,
        kernels = kernels,
        label = label,
        args = args,
        parent_layer = "antlir//antlir/bzl/linux/boot/systemd:base",
        efi_size_mb = efi_size_mb,
    )

systemd_boot = struct(
    build = _systemd_build,
)
