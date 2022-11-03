load("//antlir/bzl:build_defs.bzl", "http_file")

# This wrapper function around `native.prebuilt_python_library`
# exists because directly using `native.prebuilt_python_library`
# in BUCK causes a build error.
def prebuilt_python_library(**kwargs):
    # @lint-ignore BUCKLINT
    native.prebuilt_python_library(**kwargs)

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

    prebuilt_python_library(
        name = name,
        binary_src = ":{}-download".format(name),
        visibility = ["PUBLIC"],
        deps = deps or [],
    )
