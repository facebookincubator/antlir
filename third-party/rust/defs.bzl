load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "http_archive", "rust_library")

def third_party_rust_library(
        name,
        version,
        sha256,
        deps = None):
    http_archive(
        name = name + "--archive",
        urls = ["https://static.crates.io/crates/{name}/{name}-{version}.crate".format(
            name = name,
            version = version,
        )],
        sha256 = sha256,
        type = "tar.gz",
        strip_prefix = "{}-{}".format(name, version),
    )

    # Rust modules are usually defined in separate files for readability, but
    # it works equally fine to combine them all into one giant file with `mod`
    # blocks. This approach is taken since http_archive cannot be directly used
    # in a rust_library target's srcs
    buck_genrule(
        name = name + "--combined.rs",
        cmd = "$(exe //third-party/rust/combine:combine) $(location :{}--archive) $OUT".format(name),
        out = "out.rs",
    )
    rust_library(
        name = name,
        mapped_srcs = {
            ":{}--combined.rs".format(name): "lib.rs",
        },
        deps = deps,
    )
