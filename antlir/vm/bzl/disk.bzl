# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:btrfs.bzl", "btrfs")
load("//metalos/host_configs/tests/bzl:builder.bzl", "builders")
load("//metalos/host_configs/tests/bzl:defs.bzl", "host_config")
load(":kernel.bzl", "normalize_kernel")

def _host_config(kernel_version):
    return builders.build_host_config(
        boot_config = builders.build_boot_config(uname = kernel_version),
        provisioning_config = builders.build_provisioning_config(num_nics = 4),
        runtime_config = builders.build_runtime_config(),
    )

def _control_layer(
        name,
        kernel_version):
    host_config(name = name + "-host-config", host_config = _host_config(kernel_version))
    image.layer(
        name = name,
        features = [
            feature.ensure_dirs_exist("/run/state/metalos"),
            feature.ensure_dirs_exist("/image/initrd"),
            feature.install(
                ":{}-host-config".format(name),
                "/run/state/metalos/metalos_host_configs::host::HostConfig-current.json",
            ),
            # initrd is not actually used, so just install an empty file
            feature.install("//antlir:empty", "/image/initrd/metalos.initrd:deadbeefdeadbeefdeadbeefdeadbeef"),
        ],
        parent_layer = "//metalos/disk:control",
        visibility = [],
    )
    return ":" + name

def control_disk(
        name,
        rootfs,
        kernel,
        free_mb = 2560,  # 2.5G
        visibility = None):
    kernel = normalize_kernel(kernel)
    btrfs.new(
        name = name,
        antlir_rule = "user-internal",
        opts = btrfs.opts.new(
            default_subvol = "/volume",
            free_mb = free_mb,
            loopback_opts = image.opts(
                label = "/",
            ),
            subvols = {
                "/volume": btrfs.opts.subvol.new(
                    layer = _control_layer(name = name + "-control", kernel_version = kernel.uname),
                    writable = True,
                ),
                "/volume/image/kernel/kernel.{}:deadbeefdeadbeefdeadbeefdeadbeef".format(kernel.uname): btrfs.opts.subvol.new(
                    layer = kernel.derived_targets.image,
                ),
                "/volume/image/rootfs/metalos:deadbeefdeadbeefdeadbeefdeadbeef": btrfs.opts.subvol.new(
                    layer = rootfs,
                ),
            },
        ),
        labels = ["vm-root-disk-with-kernel={}".format(kernel.uname)],
        visibility = visibility,
    )
