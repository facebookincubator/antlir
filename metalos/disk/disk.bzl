load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//metalos/lib/metalos_paths:metalos_paths.bzl", "metalos_paths")

def disk(
        name,
        efi_vfat,
        root_btrfs,
        visibility = None):
    image.gpt(
        name = name,
        table = [
            image.gpt_partition(
                package = efi_vfat,
                is_esp = True,
            ),
            image.gpt_partition(
                package = root_btrfs,
            ),
        ],
        disk_guid = "726f6f74-6673-696d-6700-000000000001",
        visibility = visibility,
    )

def relativize_to_control(path):
    rel = paths.relativize(
        path,
        metalos_paths.control,
    )
    if rel == metalos_paths.control:
        return "/"
    return rel
