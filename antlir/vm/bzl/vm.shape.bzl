# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")
load(":kernel.shape.bzl", "kernel_t")

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

# A disk device type.  The `package` attribute of this shape must be either
# an `image.layer` target that will be transiently packaged via `package.new`
# or an existing `package.new` target.
disk_t = shape.shape(
    package = target_t,
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
    initrd = target_t,
    # The kernel to boot the vm with
    kernel = shape.field(kernel_t),
    # Append extra kernel cmdline args
    append = shape.field(shape.list(str), default = []),
    # Amount of memory in mb
    mem_mb = shape.field(int, default = 4096),
    # Root disk for the VM
    disk = shape.field(disk_t),
    # Runtime details about how to run the VM
    runtime = runtime_t,
    # What label to pass to the root kernel parameter
    root_label = shape.field(
        str,
        default = "/",
    ),
)
