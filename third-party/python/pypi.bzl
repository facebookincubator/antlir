load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "http_archive", "http_file", "python_library")

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

def source_only_pypi_package(name, url, sha256, srcs, deps = None, strip_prefix = None):
    http_archive(
        name = "{}-sources".format(name),
        sha256 = sha256,
        urls = [url],
        strip_prefix = strip_prefix,
        visibility = [],
    )
    for src, dst in srcs.items():
        buck_genrule(
            name = "{}-sources/{}".format(name, src),
            out = dst,
            cmd = "cp $(location :{}-sources)/{} $OUT".format(name, src),
        )
    python_library(
        name = name,
        srcs = {
            ":{}-sources/{}".format(name, src): dst
            for src, dst in srcs.items()
        },
        deps = deps or [],
    )
