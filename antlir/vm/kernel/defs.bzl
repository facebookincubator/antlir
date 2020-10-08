load("//antlir/bzl:oss_shim.bzl", "default_vm_image", "python_binary", "python_library", "third_party")

def create_kernel_vm_targets(kernel):
    """
    Create a kernel `-vm` and a bare '{uname}' target for each known
    kernel.
    """

    # This wraps up the necessary kernel artifacts into a python library
    # that is imported into `antlir.vm`.
    # Future: replace with a kernel_t shape.
    python_library(
        name = "{}-vm".format(kernel.uname),
        base_module = "antlir.vm",
        resources = {
            "//antlir/vm/initrd:{}-initrd".format(kernel.uname): "initrd",
            kernel.vmlinuz: "vmlinuz",
            kernel.modules: "modules",
        },
        antlir_rule = "user-internal",
    )

    python_binary(
        name = kernel.uname,
        # Needed so that we can properly find the image resource that is
        # bundled with the binary.
        base_module = "antlir.vm",
        main_module = "antlir.vm.run",
        par_style = "xar",
        deps = [
            ":{}-vm".format(kernel.uname),
            "//antlir/vm:run",
            third_party.library("click", platform = "python"),
        ],
        resources = {
            default_vm_image.package: "image",
        },
    )
