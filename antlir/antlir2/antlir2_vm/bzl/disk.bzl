# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":types.bzl", "DiskInfo")

def _disk_impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.base_image and ctx.attrs.free_mib <= 0:
        fail(
            "Either base_image or free_mib must be set. \
            An empty disk of zero size is invalid.",
        )
    if ctx.attrs.interface == "nvme":
        nvme_num_namespaces = ctx.attrs.nvme_num_namespaces if ctx.attrs.nvme_num_namespaces != None else 1
        if nvme_num_namespaces <= 0:
            fail("nvme_num_namespaces must be greater than 0")
    else:
        nvme_num_namespaces = None
    return [
        DiskInfo(
            base_image = ctx.attrs.base_image,
            free_mib = ctx.attrs.free_mib,
            interface = ctx.attrs.interface,
            logical_block_size = ctx.attrs.logical_block_size,
            physical_block_size = ctx.attrs.physical_block_size,
            bootable = ctx.attrs.bootable,
            serial = ctx.attrs.serial,
            nvme_num_namespaces = nvme_num_namespaces,
        ),
        DefaultInfo(),
    ]

# Create a VM disk that can be passed to a VM
_vm_disk = rule(
    impl = _disk_impl,
    attrs = {
        "base_image": attrs.option(
            attrs.source(doc = "Target to raw disk image file"),
            default = None,
        ),
        "bootable": attrs.bool(default = False),
        "free_mib": attrs.int(
            default = 0,
            doc = "Additional free disk space in MiB",
        ),
        "interface": attrs.enum(
            ["virtio-blk", "nvme", "ide-hd"],
            default = "virtio-blk",
            doc = "Interface for attaching to VM",
        ),
        # buck target labels
        "labels": attrs.list(attrs.string(), default = []),
        "logical_block_size": attrs.int(default = 512),
        "nvme_num_namespaces": attrs.option(attrs.int(), default = None),
        "physical_block_size": attrs.int(default = 512),
        "serial": attrs.option(
            attrs.string(),
            default = None,
            doc = "Device serial override. By default it's automatically assigned",
        ),
    },
)
vm_disk = rule_with_default_target_platform(_vm_disk)

def _create_disk_from_package(
        *,
        name: str,
        image: str,
        **kwargs):
    """This functions take image targets and wrap them with desired properties
    to create a VM disk target that can be used by VM. `image` is expected to
    be in a disk file format that can be directly consumed by qemu. It will be
    optionally expanded by `free_mib` if requested. The rule here does
    not change the images themselves, but supply other parameters that could
    affect how the disk image is used by the VM.  """
    vm_disk(
        name = name,
        base_image = image,
        **kwargs
    )
    return ":" + name

def _create_empty_disk(
        *,
        name: str,
        size_mib: int,
        **kwargs):
    """Create an empty disk of `size` MiB"""
    _create_disk_from_package(
        name = name,
        image = "antlir//antlir:empty",
        free_mib = size_mib,
        bootable = False,
        **kwargs
    )
    return ":" + name

disk = struct(
    create_disk_from_package = _create_disk_from_package,
    create_empty_disk = _create_empty_disk,
)
