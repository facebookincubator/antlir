# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")
load("//metalos/kernel:kernel.shape.bzl", "kernel_t")

emulator_t = shape.shape(
    # The actual emulator binary to invoke
    binary = target_t,
    # Firmware to use for booting
    firmware = target_t,
    # Utility to manage disk images
    img_util = target_t,
    # Location of various roms
    roms_dir = target_t,
    # Software TPM binary to invoke
    tpm_binary = shape.field(target_t, optional = True),
)

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

connection_t = shape.shape(
    options = shape.field(shape.dict(str, str), default = {}),
)

runtime_t = shape.shape(
    # Connection details
    connection = shape.field(connection_t),

    # Details of the emulator being used to run the VM
    emulator = shape.field(emulator_t),

    # Shell commands to run before booting the VM
    sidecar_services = shape.list(str),

    # Attach a TPM software emulator
    tpm = shape.field(bool, default = False),
)

vm_opts_t = shape.shape(
    # index of the serial port of vm (starts from 0)
    serial_index = shape.field(int, default = 0),
    # Boot the VM directly from the provided disk
    boot_from_disk = shape.field(bool, default = False),
    # Number of cpus to provide
    cpus = shape.field(int, default = 1),
    # Flag to mount the kernel.artifacts.devel layer into the vm at runtime.
    # Future: This should be a runtime_mount defined in the image layer itself
    # instead of being part of the vm_opts_t.
    devel = shape.field(bool, default = False),
    # The initrd to boot the vm with.  This target is always derived
    # from the provided kernel version since the initrd must contain
    # modules that match the booted kernel.
    initrd = shape.field(target_t, optional = True),
    # The kernel to boot the vm with
    kernel = shape.field(kernel_t, optional = True),
    # Append extra kernel cmdline args
    append = shape.field(shape.list(str), default = []),
    # Amount of memory in mb
    mem_mb = shape.field(int, default = 4096),
    # All disks attached to the vm
    disks = shape.list(disk_t),
    # Runtime details about how to run the VM
    runtime = runtime_t,
    # What label to pass to the root kernel parameter
    root_label = shape.field(
        str,
        default = "/",
    ),
    # Add ability to create multiple Network Input/Output Cards
    nics = shape.field(int, default = 1),
)
