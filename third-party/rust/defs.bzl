load(
    "//antlir/bzl:oss_shim.bzl",
    "buck_genrule",
    "http_archive",
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
    if not native.rule_exists(name):
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

    for target, src in mapped_srcs.items():
        src_targets[extract_buildscript_src(target)] = src

    # src_targets.update(mapped_srcs)

    rust_library(
        name = name,
        srcs = [],
        mapped_srcs = src_targets,
        **kwargs
    )

def third_party_rust_binary(name, archive, srcs, mapped_srcs = None, **kwargs):
    src_targets = {}
    for src in srcs:
        src = src.replace("vendor/", "")
        src_targets[_extract_file(archive, src)] = src

    kwargs.pop("proc_macro")

    rust_binary(
        name = name,
        srcs = [],
        mapped_srcs = src_targets,
        **kwargs
    )

    if kwargs["crate"] == "build_script_build":
        buck_genrule(
            name = name + "-args",
            out = "args",
            cmd = "$(exe :{}) | $(exe :buildrs-rustc-flags-filter) > $OUT".format(name),
        )

def extract_buildscript_src(target):
    buildscript_srcs, src = target.rsplit("=", 1)
    if not buildscript_srcs.startswith("//third-party/rust:"):
        fail("buildscript-srcs must start with //third-party/rust:")
    buildscript_srcs = buildscript_srcs[len("//third-party/rust:"):]
    if not native.rule_exists(buildscript_srcs):
        buildscript = buildscript_srcs[:-len("-srcs")]
        buck_genrule(
            name = buildscript_srcs,
            out = ":",
            cmd = "mkdir -p $OUT; OUT_DIR=$OUT TARGET=x86_64-unknown-linux-gnu $(exe :{})".format(buildscript),
        )
    buck_genrule(
        name = buildscript_srcs + "=" + src,
        out = "unused",
        cmd = "cp $(location :{})/{} $OUT".format(buildscript_srcs, src),
    )
    return ":" + buildscript_srcs + "=" + src
