# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_rust_test")

package_feature = enum(
    "dot_meta",
    "hardlink_ino_eq",
    "xattr",
)

_feature_deps = {
    package_feature("dot_meta"): [
        "//antlir/buck/buck_label:buck_label",
    ],
    package_feature("xattr"): [
        "xattr",
        "maplit",
        "//antlir/antlir2/libcap:libcap",
    ],
}

def rust_test_attrs(
        *,
        stub: str,
        deps: list[str] = [],
        omit_features: list[package_feature] = [],
        rust_features: list[str] = []):
    features = [package_feature(f) for f in package_feature.values()]
    for f in omit_features:
        features.remove(f)
    deps = ["cap-std", "nix", "pretty_assertions"] + deps
    for f in features:
        deps += _feature_deps.get(f, [])

    rust_features = list(rust_features) + [f.value for f in features]
    format = native.package_name().split("/")[-1]
    rust_features.append("format_" + format)
    return {
        "deps": deps,
        "features": rust_features,
        "mapped_srcs": {
            "//antlir/antlir2/test_images/package:standard_tests.rs": "src/lib.rs",
            stub: "src/stub.rs",
        },
    }

def test_in_layer(
        *,
        name: str,
        stub: str,
        layer_features,
        omit_package_features = [],
        deps: list[str] = []):
    image.layer(
        name = name + "-layer",
        features = [feature.rpms_install(rpms = ["basesystem"])] + layer_features,
    )
    image_rust_test(
        name = name,
        layer = ":{}-layer".format(name),
        **rust_test_attrs(
            stub = stub,
            deps = deps,
            omit_features = omit_package_features,
        )
    )

def standard_features(
        *,
        prefix: str):
    return [
        feature.install(
            src = "//antlir/antlir2/test_images/package:antlir2-large-file-256M",
            dst = paths.join(prefix, "antlir2-large-file-256M"),
        ),
        feature.ensure_dirs_exist(dirs = paths.join(prefix, "default-dir")),
        feature.install_text(
            dst = paths.join(prefix, "only-readable-by-root"),
            mode = 0o000,
            text = "Only readable by root",
        ),
        feature.install_text(
            dst = paths.join(prefix, "default-dir/executable"),
            mode = "a+rx",
            text = "#!/bin/bash\necho hello",
        ),
        feature.install_text(
            dst = paths.join(prefix, "i-am-owned-by-nonstandard"),
            group = 43,
            text = "42:43",
            user = 42,
        ),
        feature.install_text(
            dst = paths.join(prefix, "i-have-xattrs"),
            text = "xattrs are cool",
            xattrs = {
                "user.baz": "qux",
                "user.foo": "bar",
            },
        ),
        feature.install(
            src = "antlir//antlir:empty",
            dst = paths.join(prefix, "i-have-caps"),
            xattrs = {
                "security.capability": "0sAQAAAoAAAAAAAAAAAAAAAAAAAAA=",
            },
        ),
        feature.ensure_file_symlink(
            link = paths.join(prefix, "absolute-file-symlink"),
            target = paths.join(prefix, "default-dir/executable"),
        ),
        feature.ensure_file_symlink(
            link = paths.join(prefix, "default-dir/relative-file-symlink"),
            target = "executable",
        ),
        feature.ensure_dir_symlink(
            link = paths.join(prefix, "absolute-dir-symlink"),
            target = paths.join(prefix, "default-dir"),
        ),
        feature.ensure_dir_symlink(
            link = paths.join(prefix, "relative-dir-symlink"),
            target = "default-dir",
        ),
        feature.ensure_dirs_exist(dirs = paths.join(prefix, "hardlink")),
        feature.install_text(
            dst = paths.join(prefix, "hardlink/hello"),
            text = "Hello world\n",
        ),
        feature.hardlink(
            link = paths.join(prefix, "hardlink/aloha"),
            target = paths.join(prefix, "hardlink/hello"),
        ),
    ]
