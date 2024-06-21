load("//antlir/bzl:build_defs.bzl", "cpp_library")

def third_party_rust_cxx_library(name, **kwargs):
    cpp_library(name = name, **kwargs)
