load("//fs_image/bzl:image.bzl", "image")
load("//fs_image/bzl:oss_shim_vm.bzl", "image_vm_cpp_unittest", "image_vm_python_unittest")

def cpp_container_and_vm_test(
        name,
        layer,
        boot = False,
        kernel_opts = None,
        run_as_user = "nobody",
        visibility = None,
        hostname = None,
        env = None,
        **cpp_unittest_kwargs):
    if env == None:
        env = {}

    # image_vm_cpp_unittest is not yet available in OSS
    if image_vm_cpp_unittest:
        image_vm_cpp_unittest(
            name = name + "-in-vm",
            layer = layer,
            kernel_opts = kernel_opts,
            visibility = visibility,
            env = env,
            **cpp_unittest_kwargs
        )
    image.cpp_unittest(
        name = name,
        layer = layer,
        boot = boot,
        run_as_user = run_as_user,
        visibility = visibility,
        hostname = hostname,
        env = env,
        **cpp_unittest_kwargs
    )

def python_container_and_vm_test(
        name,
        layer,
        boot = False,
        kernel_opts = None,
        run_as_user = "nobody",
        visibility = None,
        hostname = None,
        env = None,
        **python_unittest_kwargs):
    if env == None:
        env = {}

    # image_vm_python_unittest is not yet available in OSS
    if image_vm_python_unittest:
        image_vm_python_unittest(
            name = name + "-in-vm",
            layer = layer,
            kernel_opts = kernel_opts,
            visibility = visibility,
            env = env,
            **python_unittest_kwargs
        )

    image.python_unittest(
        name = name,
        layer = layer,
        boot = boot,
        run_as_user = run_as_user,
        visibility = visibility,
        hostname = hostname,
        env = env,
        **python_unittest_kwargs
    )
