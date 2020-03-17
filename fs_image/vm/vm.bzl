load("//fs_image/bzl:oss_shim.bzl", "python_library")

def kernel_vm(name, kernel):
    python_library(
        name = name,
        base_module = "fs_image.vm",
        deps = [
            "//fs_image/vm:vm",
        ],
        resources = {
            kernel.vmlinuz: "vmlinuz",
            kernel.initrd: "initrd",
            kernel.modules: "modules",
        },
    )
