# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Describes a writable disk for the VM. It can be built from some base image, or
# start empty with a specified size.
DiskInfo = provider(fields = {
    "additional_size_mib": "Grow the disk by specified size",
    "base_image": "Base image for the disk. If None, the disk will be empty",
    "bootable": "True if the disk is bootable",
    "interface": "Interface of the disk",
    "logical_block_size": "Logical block size of the disk",
    "physical_block_size": "Physical block size of the disk",
})
