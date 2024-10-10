# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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

def _rust_test_attrs(
        *,
        stub: str,
        deps: list[str] = [],
        omit_features: list[package_feature] = []):
    features = [package_feature(f) for f in package_feature.values()]
    for f in omit_features:
        features.remove(f)
    deps = ["cap-std", "nix"] + deps
    for f in features:
        deps += _feature_deps.get(f, [])
    return {
        "deps": deps,
        "features": [f.value for f in features],
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
        **_rust_test_attrs(
            stub = stub,
            deps = deps,
            omit_features = omit_package_features,
        )
    )
