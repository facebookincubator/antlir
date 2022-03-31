load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/package:defs.bzl", "package")

IMAGE_DIR = "/image"

RUN_DIR = "/run"

def build_root_disk(
        name,
        root_size_mb,
        efi_layer_name,
        efi_fat_size):
    image.layer(
        name = "root-disk-layer",
        flavor = "centos8",
        features = [
            # Image directories
            image.ensure_dirs_exist(IMAGE_DIR),
            image.ensure_subdirs_exist(IMAGE_DIR, "rootfs"),
            image.ensure_subdirs_exist(IMAGE_DIR, "kernel"),

            # Runtime directories
            image.ensure_dirs_exist(RUN_DIR),
            image.ensure_subdirs_exist(RUN_DIR, "boot"),
            image.ensure_subdirs_exist(RUN_DIR, "scratch"),
            image.ensure_subdirs_exist(RUN_DIR, "state"),
        ],
        visibility = [
            "//metalos/...",
        ],
    )

    package.new(
        name = "metalos-root-package",
        layer = ":root-disk-layer",
        format = "btrfs",
        loopback_opts = image.opts(
            label = "/",
            writable_subvolume = True,
            size_mb = root_size_mb,
        ),
    )

    # Currently unused boot and efi
    image.layer(
        name = "empty-layer-boot-or-efi",
        flavor = "centos8",
        features = [
            image.ensure_dirs_exist("/PLACEHOLDER"),
        ],
        visibility = [],
    )

    package.new(
        name = "efi-package",
        layer = ":" + efi_layer_name,
        format = "vfat",
        loopback_opts = image.opts(
            fat_size = efi_fat_size,
            size_mb = 256,
            label = "metalos-efi",
        ),
        visibility = [],
    )

    package.new(
        name = "boot-package",
        layer = ":empty-layer-boot-or-efi",
        format = "btrfs",
        loopback_opts = image.opts(
            size_mb = 512,
            label = "metalos-boot",
        ),
        visibility = [],
    )

    image.gpt(
        name = name,
        table = [
            image.gpt_partition(
                package = ":efi-package",
                is_esp = True,
            ),
            image.gpt_partition(
                package = ":boot-package",
            ),
            image.gpt_partition(
                package = ":metalos-root-package",
            ),
        ],
        disk_guid = "726f6f74-6673-696d-6700-000000000001",
        visibility = ["//metalos/..."],
    )
