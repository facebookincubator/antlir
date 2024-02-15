# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "python_library", "rust_library")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")

def antlir_rust_extension(
        name,
        srcs,
        typestub,
        deps = (),
        labels = (),
        rust_visibility = (),
        **kwargs):
    deps = list(deps)
    deps.append("pyo3")
    labels = list(labels)
    module_name = native.package_name().replace("/", ".") + "." + name
    labels.append("antlir-rust-extension")
    labels.append("antlir-rust-extension=" + structs.as_json(struct(
        rust_crate = name,
        module = module_name,
    )))
    rust_library(
        name = name + "-rust",
        crate = name,
        srcs = srcs,
        deps = deps,
        visibility = [antlir_dep("rust:native_antlir_impl-lib")] + (rust_visibility or []),
        labels = labels,
        unittests = False,
        **kwargs
    )
    python_library(
        name = name,
        srcs = {
            antlir_dep("rust:trigger_rust_module_init.py"): name + ".py",
            typestub: name + ".pyi",
        },
        deps = [antlir_dep("rust:rust")],
    )
