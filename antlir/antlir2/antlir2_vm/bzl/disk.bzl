# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":types.bzl", "DiskInfo")

_COMMON_DISK_ATTRS = {
    "base_image": attrs.option(
        attrs.source(doc = "Target to raw disk image file"),
        default = None,
    ),
    "bootable": attrs.bool(default = False),
    "free_mib": attrs.int(
        default = 0,
        doc = "Additional free disk space in MiB",
    ),
    # buck target labels
    "labels": attrs.list(attrs.string(), default = []),
    "logical_block_size": attrs.int(default = 512),
    "physical_block_size": attrs.int(default = 512),
    "serial": attrs.option(
        attrs.string(),
        default = None,
        doc = "Device serial override. By default it's automatically assigned",
    ),
}

def _validate_has_image_or_size(ctx):
    if not ctx.attrs.base_image and ctx.attrs.free_mib <= 0:
        fail(
            "Either base_image or free_mib must be set. \
            An empty disk of zero size is invalid.",
        )

def _common_disk_info(ctx, *, interface, nvme = None, iscsi = None):
    return DiskInfo(
        base_image = ctx.attrs.base_image,
        free_mib = ctx.attrs.free_mib,
        interface = interface,
        logical_block_size = ctx.attrs.logical_block_size,
        physical_block_size = ctx.attrs.physical_block_size,
        bootable = ctx.attrs.bootable,
        serial = ctx.attrs.serial,
        nvme = nvme,
        iscsi = iscsi,
    )

# virtio-blk disk

def _virtio_blk_disk_impl(ctx: AnalysisContext) -> list[Provider]:
    _validate_has_image_or_size(ctx)
    return [_common_disk_info(ctx, interface = "virtio-blk"), DefaultInfo()]

_virtio_blk_disk = rule(
    impl = _virtio_blk_disk_impl,
    attrs = _COMMON_DISK_ATTRS,
)
virtio_blk_disk = rule_with_default_target_platform(_virtio_blk_disk)

# IDE hard disk (SATA via AHCI)

def _ide_hd_disk_impl(ctx: AnalysisContext) -> list[Provider]:
    _validate_has_image_or_size(ctx)
    return [_common_disk_info(ctx, interface = "ide-hd"), DefaultInfo()]

_ide_hd_disk = rule(
    impl = _ide_hd_disk_impl,
    attrs = _COMMON_DISK_ATTRS,
)
ide_hd_disk = rule_with_default_target_platform(_ide_hd_disk)

# NVMe disk with namespace configuration

def _nvme_disk_impl(ctx: AnalysisContext) -> list[Provider]:
    _validate_has_image_or_size(ctx)
    if ctx.attrs.num_namespaces <= 0:
        fail("num_namespaces must be greater than 0")
    return [
        _common_disk_info(
            ctx,
            interface = "nvme",
            nvme = struct(
                num_namespaces = ctx.attrs.num_namespaces,
            ),
        ),
        DefaultInfo(),
    ]

_nvme_disk = rule(
    impl = _nvme_disk_impl,
    attrs = _COMMON_DISK_ATTRS
    | {
        "num_namespaces": attrs.int(
            default = 1,
            doc = "Number of NVMe namespaces",
        ),
    },
)
nvme_disk = rule_with_default_target_platform(_nvme_disk)

# iSCSI disk: the VM infrastructure starts a local tgtd daemon that exports the
# backing file. Connection details (portal, target IQN, LUN) are determined at
# runtime, so this rule only carries the common backing-storage attrs.

def _iscsi_disk_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        _common_disk_info(
            ctx,
            interface = "iscsi",
            iscsi = struct(
                ibft = ctx.attrs.ibft,
            ),
        ),
        DefaultInfo(),
    ]

_iscsi_disk = rule(
    impl = _iscsi_disk_impl,
    attrs = _COMMON_DISK_ATTRS
    | {
        "ibft": attrs.bool(
            default = False,
            doc = "Advertise this iSCSI disk via an iBFT ACPI table. At most one disk may set this.",
        ),
    },
)
iscsi_disk = rule_with_default_target_platform(_iscsi_disk)

_INTERFACE_RULES = {
    "ide-hd": ide_hd_disk,
    "iscsi": iscsi_disk,
    "nvme": nvme_disk,
    "virtio-blk": virtio_blk_disk,
}

def vm_disk(*, name: str, interface: str = "virtio-blk", **kwargs):
    """Backward-compatible macro that dispatches to the per-interface rule."""
    rule_fn = _INTERFACE_RULES.get(interface)
    if rule_fn == None:
        fail("Unknown disk interface: {}".format(interface))
    rule_fn(name = name, **kwargs)

def _create_disk_from_package(*, name: str, image: str, interface: str = "virtio-blk", **kwargs):
    """This functions take image targets and wrap them with desired properties
    to create a VM disk target that can be used by VM. `image` is expected to
    be in a disk file format that can be directly consumed by qemu. It will be
    optionally expanded by `free_mib` if requested. The rule here does
    not change the images themselves, but supply other parameters that could
    affect how the disk image is used by the VM."""
    vm_disk(name = name, base_image = image, interface = interface, **kwargs)
    return ":" + name

def _create_empty_disk(*, name: str, size_mib: int, interface: str = "virtio-blk", **kwargs):
    """Create an empty disk of `size` MiB"""
    _create_disk_from_package(name = name, image = "antlir//antlir:empty", free_mib = size_mib, bootable = False, interface = interface, **kwargs)
    return ":" + name

disk = struct(
    create_disk_from_package = _create_disk_from_package,
    create_empty_disk = _create_empty_disk,
)
