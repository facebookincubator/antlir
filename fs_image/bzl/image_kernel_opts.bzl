# Refer to image_vm_unittest.bzl for user documentation
def image_kernel_opts(
        kernel,
        install_headers = False,
        install_devel = False):
    return struct(
        kernel = kernel,
        install_headers = install_headers,
        install_devel = install_devel,
    )
