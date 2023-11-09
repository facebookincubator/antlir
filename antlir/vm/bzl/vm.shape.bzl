# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")
load("//metalos/kernel:kernel.shape.bzl", "kernel_t")

disk_interface_t = shape.enum("virtio-blk", "nvme", "ide-hd")

# A disk device type.  The `package` attribute of this shape must be an existing
# `package.new` target.
disk_t = shape.shape(
    package = target_t,
    # additional size to add to disk image at VM runtime,
    # this might be necessary when the OS tries to dynanically
    # request more disk space (e.g creating a new GPT partition)
    additional_scratch_mb = shape.field(int, optional = True),
    interface = shape.field(disk_interface_t, default = "virtio-blk"),
    subvol = shape.field(str, optional = True),
    # root disks are built with a kernel image inside them, track what version
    # that is to ensure it matches the kernel we're trying to boot the vm with
    contains_kernel = shape.field(kernel_t, optional = True),
    serial = shape.field(str, optional = True),
    physical_block_size = shape.field(int, default = 512),
    logical_block_size = shape.field(int, default = 512),
)
