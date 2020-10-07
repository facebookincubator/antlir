load("//antlir/bzl:oss_shim.bzl", "kernel_get", "python_library")

def create_vm_targets():
    """
    Create a kernel `-vm` target for each known kernel
    """
    for uname, kernel in kernel_get.versions.items():
        python_library(
            name = "{}-vm".format(uname),
            base_module = "antlir.vm",
            resources = {
                "//antlir/vm/initrd:{}-initrd".format(kernel.uname): "initrd",
                kernel.vmlinuz: "vmlinuz",
                kernel.modules: "modules",
            },
            antlir_rule = "user-internal",
        )
