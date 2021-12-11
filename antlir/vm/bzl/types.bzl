# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:oss_shim.bzl", "kernel_get", "third_party")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/package:new.bzl", "package_new")
load(":kernel.bzl", "kernel_t", "normalize_kernel")

_vm_emulator_t = shape.shape(
    # The actual emulator binary to invoke
    binary = shape.target(),
    # Firmware to use for booting
    firmware = shape.target(),
    # Utility to manage disk images
    img_util = shape.target(),
    # Location of various roms
    roms_dir = shape.target(),
)

def _new_vm_emulator(
        binary = None,
        firmware = None,
        img_util = None,
        roms_dir = None,
        **kwargs):
    # These defaults have to be set here due to the use of the
    # `third_party.library` function.  It must be invoked inside of
    # either a rule definition or another function, it cannot be used
    # at the top-level of an included .bzl file (where the type def is).
    firmware = firmware or antlir_dep("vm:efi-code")
    binary = binary or third_party.library("qemu")
    img_util = img_util or third_party.library("qemu", "qemu-img")
    roms_dir = roms_dir or antlir_dep("vm:roms")

    return shape.new(
        _vm_emulator_t,
        binary = binary,
        firmware = firmware,
        img_util = img_util,
        roms_dir = roms_dir,
        **kwargs
    )

_vm_emulator_api = struct(
    new = _new_vm_emulator,
    t = _vm_emulator_t,
)

# A disk device type.  The `package` attribute of this shape must be either
# an `image.layer` target that will be transiently packaged via `package.new`
# or an existing `package.new` target.
_vm_disk_t = shape.shape(
    package = shape.target(),
)

def _new_vm_disk(
        package = None,
        layer = None,
        layer_size_mb = None,
        layer_label = "/"):
    if package and layer:
        fail("disk.new() accepts `package` OR `layer`, not both")

    if layer:
        # Convert the provided layer name into something that we can safely use
        # as the base for a new target name.  This is only used for the
        # vm being constructed here, so it doesn't have to be pretty.
        layer_name = layer.lstrip(":").lstrip("//").replace("/", "_").replace(":", "__")
        package_target = "{}=image.btrfs".format(layer_name)
        if not native.rule_exists(package_target):
            package_new(
                name = package_target,
                layer = layer,
                format = "btrfs",
                loopback_opts = struct(
                    label = layer_label,
                    size_mb = layer_size_mb,
                    writable_subvolume = True,
                ),
                visibility = [],
                antlir_rule = "user-internal",
            )
        package = ":" + package_target

    elif not package:
        package = REPO_CFG.artifact["vm.rootfs.btrfs"]

    return shape.new(
        _vm_disk_t,
        package = package,
    )

_vm_disk_api = struct(
    new = _new_vm_disk,
    t = _vm_disk_t,
)

_vm_connection_t = shape.shape(
    options = shape.dict(str, shape.union(str, int), default = {}),
)

def _new_vm_connection(**kwargs):
    return shape.new(
        _vm_connection_t,
        **kwargs
    )

_vm_connection_api = struct(
    new = _new_vm_connection,
    t = _vm_connection_t,
)

_vm_runtime_t = shape.shape(
    # Connection details
    connection = shape.field(_vm_connection_t),

    # Details of the emulator being used to run the VM
    emulator = shape.field(_vm_emulator_t),

    # Shell commands to run before booting the VM
    sidecar_services = shape.list(str),
)

def _new_vm_runtime(
        connection = None,
        emulator = None,
        sidecar_services = None):
    return shape.new(
        _vm_runtime_t,
        connection = connection or _new_vm_connection(),
        emulator = emulator or _new_vm_emulator(),
        sidecar_services = sidecar_services or [],
    )

_vm_runtime_api = struct(
    new = _new_vm_runtime,
    t = _vm_runtime_t,
)

_vm_opts_t = shape.shape(
    # Number of cpus to provide
    cpus = shape.field(int, default = 1),
    # Flag to mount the kernel.artifacts.devel layer into the vm at runtime.
    # Future: This should be a runtime_mount defined in the image layer itself
    # instead of being part of the vm_opts_t.
    devel = shape.field(bool, default = False),
    # The initrd to boot the vm with.  This target is always derived
    # from the provided kernel version since the initrd must contain
    # modules that match the booted kernel.
    initrd = shape.target(),
    # The kernel to boot the vm with
    kernel = shape.field(kernel_t),
    # Append extra kernel cmdline args
    append = shape.list(str, default = []),
    # Amount of memory in mb
    mem_mb = shape.field(int, default = 4096),
    # Root disk for the VM
    disk = shape.field(_vm_disk_t),
    # Runtime details about how to run the VM
    runtime = shape.field(_vm_runtime_t),
    # What label to pass to the root kernel parameter
    root_label = shape.field(
        str,
        default = "/",
    ),
)

def _new_vm_opts(
        bios = None,
        cpus = 1,
        kernel = None,
        initrd = None,
        disk = None,
        runtime = None,
        **kwargs):
    # Don't allow an invalid cpu count
    if cpus == 2:
        fail("ncpus=2 will cause kernel panic: https://fburl.com/md27i5k8")

    # Convert the (optionally) provided kernel struct into a shape type
    kernel = normalize_kernel(kernel or kernel_get.default)

    # Allow the user to provide their own initrd. Currently there is no way for
    # us to verify that this initrd will actually work with the given kernel,
    # but if someone is using this (eg, the vm appliance), assume they are
    # accepting the risks.
    # The default initrd target is derived from the kernel uname.
    initrd = initrd or "{}:{}-initrd".format(kernel_get.base_target, kernel.uname)

    disk = disk or _new_vm_disk()

    runtime = runtime or _new_vm_runtime()

    return shape.new(
        _vm_opts_t,
        cpus = cpus,
        initrd = initrd,
        kernel = kernel,
        disk = disk,
        runtime = runtime,
        **kwargs
    )

_vm_opts_api = struct(
    new = _new_vm_opts,
    t = _vm_opts_t,
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
