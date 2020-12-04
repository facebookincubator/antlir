load("//antlir/bzl:oss_shim.bzl", "http_file")

def pypi_package(
        name,
        url,
        sha256,
        deps = None):
    http_file(
        name = "{}-download".format(name),
        sha256 = sha256,
        urls = [url],
        visibility = [],
    )

    native.prebuilt_python_library(
        name = name,
        binary_src = ":{}-download".format(name),
        visibility = ["PUBLIC"],
        deps = deps or [],
    )
