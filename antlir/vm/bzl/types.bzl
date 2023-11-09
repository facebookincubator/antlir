# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:kernel_shim.bzl", "kernels")
load(":disk.bzl", "control_disk")
load(":kernel.bzl", "normalize_kernel")
load(":vm.shape.bzl", "disk_t")

def _new_vm_root_disk(
        layer = REPO_CFG.artifact["vm.rootfs.layer"],
        kernel = kernels.default,
        disk_free_mb = 0,
        interface = "virtio-blk",
        serial = None):
    kernel = normalize_kernel(kernel)

    # Convert the provided layer name into something that we can safely use as
    # the base for a new target name.  This is only used for the vm being
    # constructed here, so it doesn't have to be pretty.
    layer_name = layer.lstrip(":").lstrip("//").replace("/", "_").replace(":", "__")
    package_target = "{}=image-{}.btrfs".format(layer_name, kernel.uname)

    if not native.rule_exists(package_target):
        control_disk(
            name = package_target,
            rootfs = layer,
            kernel = kernel,
            free_mb = disk_free_mb,
            visibility = [],
        )
    package = ":" + package_target
    return disk_t(
        package = package,
        interface = interface,
        subvol = "volume",
        contains_kernel = kernel,
        serial = serial,
    )

def _new_vm_scratch_disk(
        size_mb,
        interface = "virtio-blk",
        physical_block_size = 512,
        logical_block_size = 512,
        contains_kernel = None,
        serial = None):
    return disk_t(
        package = "//antlir:empty",
        additional_scratch_mb = size_mb,
        interface = interface,
        subvol = None,
        physical_block_size = physical_block_size,
        logical_block_size = logical_block_size,
        contains_kernel = contains_kernel,
        serial = serial,
    )

def _new_vm_disk_from_package(
        package,
        interface = "virtio-blk",
        physical_block_size = 512,
        logical_block_size = 512,
        subvol = "volume",
        additional_scratch_mb = None,
        contains_kernel = None,
        serial = None):
    return disk_t(
        package = package,
        interface = interface,
        subvol = subvol,
        physical_block_size = physical_block_size,
        logical_block_size = logical_block_size,
        additional_scratch_mb = additional_scratch_mb,
        contains_kernel = contains_kernel,
        serial = serial,
    )

_vm_disk_api = struct(
    root = _new_vm_root_disk,
    scratch = _new_vm_scratch_disk,
    from_package = _new_vm_disk_from_package,
    t = disk_t,
)

# Export everything as a more structured api.
api = struct(
    disk = _vm_disk_api,
)
