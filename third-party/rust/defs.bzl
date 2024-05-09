load("//antlir/bzl:build_defs.bzl", "cpp_library")

def third_party_rust_cxx_library(name, **kwargs):
    if name.startswith("librocksdb-sys-") and name.endswith("-rocksdb"):
        kwargs["exported_linker_flags"] = ["-lstdc++"]
    cpp_library(name = name, **kwargs)
