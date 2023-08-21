# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:kernel_shim.bzl", "kernels")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":disk.bzl", "control_disk")
load(":kernel.bzl", "normalize_kernel")
load(":vm.shape.bzl", "connection_t", "disk_t", "emulator_t", "runtime_t", "vm_opts_t")

def _new_vm_emulator(
        binary = None,
        firmware = None,
        img_util = None,
        roms_dir = None,
        tpm_binary = None,
        tpm = False,
        **kwargs):
    # These defaults have to be set here due to the use of the
    # `third_party.library` function.  It must be invoked inside of
    # either a rule definition or another function, it cannot be used
    # at the top-level of an included .bzl file (where the type def is).
    firmware = firmware or antlir_dep("vm/runtime:edk2-x86_64-code.fd")
    binary = binary or antlir_dep("vm/runtime:qemu-system-x86_64")
    img_util = img_util or antlir_dep("vm/runtime:qemu-img")
    roms_dir = roms_dir or antlir_dep("vm/runtime:roms")
    tpm_binary = tpm_binary or antlir_dep("vm/runtime:swtpm")

    return emulator_t(
        binary = binary,
        firmware = firmware,
        img_util = img_util,
        roms_dir = roms_dir,
        tpm_binary = tpm_binary if tpm else None,
        **kwargs
    )

_vm_emulator_api = struct(
    new = _new_vm_emulator,
    t = emulator_t,
)

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

def _new_vm_connection(**kwargs):
    return connection_t(
        **kwargs
    )

_vm_connection_api = struct(
    new = _new_vm_connection,
    t = connection_t,
)

def _new_vm_runtime(
        connection = None,
        emulator = None,
        sidecar_services = None,
        tpm = False):
    runtime = runtime_t(
        connection = connection or _new_vm_connection(),
        emulator = emulator or _new_vm_emulator(tpm = tpm),
        sidecar_services = sidecar_services or [],
        tpm = tpm,
    )
    if tpm and not runtime.emulator.tpm_binary:
        fail("tpm=True, but emulator is missing tpm_binary")
    return runtime

_vm_runtime_api = struct(
    new = _new_vm_runtime,
    t = runtime_t,
)

def _new_vm_opts(
        cpus = 1,
        kernel = None,
        initrd = None,
        disk = None,
        disks = (),
        runtime = None,
        boot_from_disk = False,
        nics = 1,
        **kwargs):
    if boot_from_disk and initrd != None:
        fail("Can't specify `initrd` when `boot_from_disk` is True")

    if boot_from_disk and kwargs.get("append", None) != None:
        fail("Can't specify `append` when `boot_from_disk` is True")

    # Allow the user to provide their own initrd. Currently there is no way for
    # us to verify that this initrd will actually work with the given kernel,
    # but if someone is using this (eg, the vm appliance), assume they are
    # accepting the risks.
    # The default initrd target is derived from the kernel uname.
    if not boot_from_disk:
        # Convert the (optionally) provided kernel struct into a shape type
        kernel = normalize_kernel(kernel or kernels.default)
        initrd = initrd or antlir_dep("vm/initrd:{}-initrd".format(kernel.uname))

    if disk and not disks:
        disks = [disk]
    elif disk and disks:
        disks = [disk] + list(disks)
    elif not disk and not disks:
        disks = [_vm_disk_api.root(kernel = kernel)]

    if not boot_from_disk:
        root_disk = disks[0]
        if not root_disk.contains_kernel:
            fail("root disk must be built with a kernel image")
        elif root_disk.contains_kernel != kernel:
            fail("kernel installed in root disk must match boot kernel ({} != {})".format(root_disk.contains_kernel.uname, kernel.uname))

    runtime = runtime or _new_vm_runtime()

    # Sanity check NICs
    if nics < 1 or nics > 69:
        fail("NIC count must be >= 1 <= 69")

    return vm_opts_t(
        cpus = cpus,
        initrd = initrd,
        kernel = kernel,
        disks = disks,
        runtime = runtime,
        boot_from_disk = boot_from_disk,
        nics = nics,
        **kwargs
    )

_vm_opts_api = struct(
    new = _new_vm_opts,
    t = vm_opts_t,
)

# Export everything as a more structured api.
api = struct(
    opts = _vm_opts_api,
    disk = _vm_disk_api,
    # API for runtime options describing how the VM should be run
    runtime = _vm_runtime_api,
    connection = _vm_connection_api,
    emulator = _vm_emulator_api,
)
