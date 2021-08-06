load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")
load(
    "//antlir/bzl:oss_shim.bzl",
    "buck_genrule",
    "rust_binary",
    "rust_library",
)

def archive(name, sha256, url):
    http_archive(
        name = name,
        urls = [url],
        sha256 = sha256,
        type = "tar.gz",
    )

def _extract_file(archive, src):
    name = archive[1:] + "/" + src
    if not native.rule_exists(":" + name):
        buck_genrule(
            name = "{}/{}".format(archive[1:], src),
            out = src,
            cmd = "mkdir -p `dirname $OUT`; cp $(location {})/{} $OUT".format(archive, src),
        )
    return ":" + name

def third_party_rust_library(name, archive, srcs, mapped_srcs = None, **kwargs):
    src_targets = {}
    for src in srcs:
        src = src.replace("vendor/", "")
        src_targets[_extract_file(archive, src)] = src

    # src_targets.update(mapped_srcs)

    kwargs.pop("rustc_flags")

    rust_library(
        name = name,
        srcs = [],
        mapped_srcs = src_targets,
        crate_root = crate_root,
        **kwargs
    )

def third_party_rust_binary(name, archive, srcs, download, mapped_srcs = None, **kwargs):
    src_targets = {}
    for src in srcs:
        src = src.replace("vendor/", "")
        src_targets[_extract_file(archive, src)] = src

    rust_binary(
        name = name,
        srcs = [],
        mapped_srcs = src_targets,
        crate_root = crate_root,
        **kwargs
    )
