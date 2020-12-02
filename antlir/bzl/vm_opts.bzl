load("//antlir/vm:kernel.bzl", "kernel_t", "normalize_kernel")
load(":image.bzl", "image")
load(":oss_shim.bzl", "default_vm_image", "kernel_get", "third_party")
load(":shape.bzl", "shape")

vm_opts_t = shape.shape(
    # Bios to use for booting
    bios = shape.target(),
    # Number of cpus to provide
    cpus = shape.field(int, default = 1),
    # Flag to mount the kernel.artifacts.devel layer into the vm at runtime.
    # Future: This should be a runtime_mount defined in the image layer itself
    # instead of being part of the vm_opts_t.
    devel = shape.field(bool, default = False),
    # The actual emulator to invoke
    emulator = shape.target(),
    # Provide a directory from where the emulator can load firmware roms
    emulator_roms_dir = shape.target(default = "//antlir/vm:roms"),
    # The initrd to boot the vm with.  This target is always derived
    # from the provided kernel version since the initrd must contain
    # modules that match the booted kernel.
    initrd = shape.target(),
    # The kernel to boot the vm with
    kernel = shape.field(kernel_t),
    # Amount of memory in mb
    mem_mb = shape.field(int, default = 4096),
    # Rootfs image for the vm.  This can be optionally derived from an
    # `image.layer` target provided to the `new_vm_opts` constructor.
    # Otherwise, this must be a target to an `image.package` that
    # builds an `image.layer` into a seed device enabled loopback.
    rootfs_image = shape.target(),
)

def new_vm_opts(
        bios = None,
        cpus = 1,
        emulator = None,
        kernel = None,
        layer = None,
        rootfs_image = None,
        **kwargs):
    # Don't allow an invalid cpu count
    if cpus == 2:
        fail("ncpus=2 will cause kernel panic: https://fburl.com/md27i5k8")

    if rootfs_image and layer:
        fail("Cannot use `rootfs_image` and `layer` together")

    # Convert the (optionally) provided kernel struct into a shape type
    kernel = normalize_kernel(kernel or kernel_get.default)

    # These defaults have to be set here due to the use of the
    # `third_party.library` function.  It must be invoked inside of
    # either a rule definition or another function, it cannot be used
    # at the top-level of an included .bzl file (where the type def is).
    bios = bios or third_party.library("qemu", "share/qemu/bios-256k.bin")
    emulator = emulator or third_party.library("qemu")

    # The initrd target is derived from the kernel uname.
    # Note: In the future we would like to support user provided initrds.
    # However, the initrd must match the kernel uname and since we don't
    # have a good way to verify that this is the case, we will instea
    # not allow it at this time.
    initrd = "{}:{}-initrd".format(kernel_get.base_target, kernel.uname)

    # If the vm is using the default rootfs layer, we can use the
    # pre-packaged seed device and save lots of build time
    # Otherwise we have to build a seed device using the layer
    # the user provided
    if layer and layer != default_vm_image.layer:
        # Convert the provided layer name into something that we can safely use
        # as the base for a new target name.  This is only used for the
        # vm being constructed here, so it doesn't have to be pretty.
        layer_name = layer.lstrip(":").lstrip("//").replace("/", "_").replace(":", "__")
        seed_image_target = "{}=seed.btrfs".format(layer_name)
        if not native.rule_exists(seed_image_target):
            image.package(
                name = seed_image_target,
                layer = layer,
                seed_device = True,
                writable_subvolume = True,
                visibility = [],
                antlir_rule = "user-internal",
            )
        rootfs_image = ":" + seed_image_target
    else:
        rootfs_image = default_vm_image.package

    return shape.new(
        vm_opts_t,
        bios = bios,
        cpus = cpus,
        emulator = emulator,
        initrd = initrd,
        kernel = kernel,
        rootfs_image = rootfs_image,
        **kwargs
    )
