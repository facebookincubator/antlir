# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":types.bzl", "DiskInfo")

def _disk_impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.base_image and ctx.attrs.additional_mib <= 0:
        fail(
            "Either base_image or additional_mib must be set. \
            An empty disk of zero size is invalid.",
        )
    return [
        DiskInfo(
            base_image = ctx.attrs.base_image[DefaultInfo].default_outputs[0],
            additional_mib = ctx.attrs.additional_mib,
            interface = ctx.attrs.interface,
            logical_block_size = ctx.attrs.logical_block_size,
            physical_block_size = ctx.attrs.physical_block_size,
            bootable = ctx.attrs.bootable,
            serial = ctx.attrs.serial,
        ),
        DefaultInfo(),
    ]

# Create a VM disk that can be passed to a VM
_vm_disk = rule(
    impl = _disk_impl,
    attrs = {
        "additional_mib": attrs.int(
            default = 0,
            doc = "Additional free disk space in MiB",
        ),
        "base_image": attrs.option(
            attrs.dep(doc = "Target to raw disk image file"),
            default = None,
        ),
        "bootable": attrs.bool(),
        "interface": attrs.enum(
            ["virtio-blk", "nvme", "ide-hd"],
            default = "virtio-blk",
            doc = "Interface for attaching to VM",
        ),
        "logical_block_size": attrs.int(default = 512),
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
        name: str,
        image: str,
        additional_mib: int = 0,
        bootable: bool = False,
        interface: str = "virtio-blk",
        logical_block_size: int = 512,
        physical_block_size: int = 512,
        serial: str | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    """This functions take image targets and wrap them with desired properties
    to create a VM disk target that can be used by VM. `image` is expected to
    be in a disk file format that can be directly consumed by qemu. It will be
    optionally expanded by `additional_mib` if requested. The rule here does
    not change the images themselves, but supply other parameters that could
    affect how the disk image is used by the VM.  """
    vm_disk(
        name = name,
        base_image = image,
        bootable = bootable,
        additional_mib = additional_mib,
        interface = interface,
        logical_block_size = logical_block_size,
        physical_block_size = physical_block_size,
        serial = serial,
        visibility = visibility,
        **kwargs
    )
    return ":" + name

def _create_empty_disk(
        name: str,
        size_mib: int,
        interface: str = "virtio-blk",
        logical_block_size: int = 512,
        physical_block_size: int = 512,
        serial: str | None = None,
        visibility: list[str] | None = None):
    """Create an empty disk of `size` MiB"""
    _create_disk_from_package(
        name = name,
        image = "antlir//antlir:empty",
        additional_mib = size_mib,
        bootable = False,
        interface = interface,
        logical_block_size = logical_block_size,
        physical_block_size = physical_block_size,
        serial = serial,
        visibility = visibility,
    )
    return ":" + name

disk = struct(
    create_disk_from_package = _create_disk_from_package,
    create_empty_disk = _create_empty_disk,
)
