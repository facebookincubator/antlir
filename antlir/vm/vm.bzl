load("//antlir/bzl:oss_shim.bzl", "python_library")

def vm(name, kernel):
    python_library(
        name = name,
        base_module = "antlir.vm",
        deps = [
            "//antlir/vm:vm",
        ],
        resources = {
            kernel.vmlinuz: "vmlinuz",
            kernel.initrd: "initrd",
            kernel.modules: "modules",
        },
    )
