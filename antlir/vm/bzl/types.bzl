# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:kernel_shim.bzl", "kernels")
load("//antlir/bzl:oss_shim.bzl", "third_party")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/package:btrfs.bzl", "btrfs")
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
    firmware = firmware or antlir_dep("vm:efi-code")
    binary = binary or third_party.library("qemu")
    img_util = img_util or third_party.library("qemu", "qemu-img")
    roms_dir = roms_dir or antlir_dep("vm:roms")
    tpm_binary = tpm_binary or third_party.library("swtpm", "bin", "antlir")

    return shape.new(
        emulator_t,
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

def _new_vm_disk(
        package = None,
        layer = None,
        layer_free_mb = 0,
        layer_label = "/",
        additional_scratch_mb = None,
        interface = "virtio-blk",
        subvol = "volume"):
    if package and layer:
        fail("disk.new() accepts `package` OR `layer`, not both")

    if layer:
        # Convert the provided layer name into something that we can safely use
        # as the base for a new target name.  This is only used for the
        # vm being constructed here, so it doesn't have to be pretty.
        layer_name = layer.lstrip(":").lstrip("//").replace("/", "_").replace(":", "__")
        package_target = "{}=image.btrfs".format(layer_name)
        if not native.rule_exists(package_target):
            btrfs.new(
                name = package_target,
                opts = btrfs.opts.new(
                    subvols = {
                        "/" + subvol: btrfs.opts.subvol.new(
                            layer = layer,
                            writable = True,
                        ),
                    },
                    free_mb = layer_free_mb,
                    loopback_opts = struct(
                        label = layer_label,
                    ),
                ),
                visibility = [],
                antlir_rule = "user-internal",
            )
        package = ":" + package_target

    elif not package:
        package = REPO_CFG.artifact["vm.rootfs.btrfs"]

    return shape.new(
        disk_t,
        package = package,
        additional_scratch_mb = additional_scratch_mb,
        interface = interface,
        subvol = subvol,
    )

_vm_disk_api = struct(
    new = _new_vm_disk,
    t = disk_t,
)

def _new_vm_connection(**kwargs):
    return shape.new(
        connection_t,
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
    runtime = shape.new(
        runtime_t,
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
        runtime = None,
        boot_from_disk = False,
        **kwargs):
    # Don't allow an invalid cpu count
    if cpus == 2:
        fail("ncpus=2 will cause kernel panic: https://fburl.com/md27i5k8")

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

    disk = disk or _new_vm_disk()

    runtime = runtime or _new_vm_runtime()

    return shape.new(
        vm_opts_t,
        cpus = cpus,
        initrd = initrd,
        kernel = kernel,
        disk = disk,
        runtime = runtime,
        boot_from_disk = boot_from_disk,
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
