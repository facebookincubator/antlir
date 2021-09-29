# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "rust_library")

def thrift_library(
        name,
        languages,
        thrift_srcs,
        rust_crate_name = None,
        rust_deps = None,
        rust_features = None,
        rust_include_srcs = None,
        rust_unittests = False,
        thrift_rust_options = None,
        deps = None):
    if languages != ["rust"]:
        fail("thrift_library only supports rust")
    if deps:
        fail("thrift_library does not support deps")
    image.layer(
        name = "{}--src".format(name),
        parent_layer = "//images/appliance:rc-build-appliance",
        features = [
            image.ensure_dirs_exist("/src"),
            image.ensure_subdirs_exist("/src", "thrift"),
            image.ensure_subdirs_exist("/src", "rust"),
            image.ensure_dirs_exist("/out"),
            [
                image.install(src, "/src/thrift/{}".format(src))
                for src in thrift_srcs.keys()
            ],
            [
                image.install(src, "/src/rust/{}".format(src))
                for src in rust_include_srcs
            ],
        ],
    )
    thrift_options = (thrift_rust_options or "").split(",")

    image.genrule_layer(
        name = "{}--compile".format(name),
        parent_layer = ":{}--src".format(name),
        user = "root",
        cmd = [
            "/usr/bin/thrift1",
            "--gen",
            "mstch_rust:{}".format(",".join(thrift_options)),
            "-out",
            "/out",
        ] + [
            "/src/thrift/{}".format(src)
            for src in thrift_srcs.keys()
        ],
        rule_type = "thrift",
        antlir_rule = "user-internal",
    )

    # TODO: implement rust_include_srcs with an option to mstch_rust when we get
    # a newer version of fbthrift (in a newer version of fedora)
    includes = "\n".join(["echo 'include!(\"{}\");' >> $OUT".format(src) for src in rust_include_srcs])

    buck_genrule(
        name = "{}--lib.rs".format(name),
        out = "lib.rs",
        cmd = """
            binary_path=( $(exe //antlir:find-built-subvol) )
            layer_loc="$(location {layer})"
            sv_path=\\$( "${{binary_path[@]}}" "$layer_loc" )
            cp "$sv_path/out/lib.rs" --no-clobber "$OUT"
            {includes}
        """.format(layer = ":{}--compile".format(name), includes = includes),
    )

    rust_library(
        name = "{}-rust".format(name),
        mapped_srcs = {
            ":{}--lib.rs".format(name): "lib.rs",
        },
        srcs = rust_include_srcs,
        deps = rust_deps,
        features = rust_deps,
    )
