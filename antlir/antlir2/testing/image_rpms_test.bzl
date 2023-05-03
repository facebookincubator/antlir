# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")

def _impl(ctx: "context") -> ["provider"]:
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            ctx.attrs.image_rpms_test[RunInfo],
            "rpm-names",
            ctx.attrs.src,
        )),
    ]

_script = rule(
    impl = _impl,
    attrs = {
        "image_rpms_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_rpms_test:image-rpms-test")),
        "src": attrs.source(),
    },
)

def image_test_rpm_names(name: str.type, src: str.type, layer: str.type, **kwargs):
    _script(
        name = name + "--script",
        src = src,
    )
    image.layer(
        name = name + "--layer",
        # This must have 'rpm' installed already, so use the build appliance to
        # query the layer-under-test instead of relying on the image to have the
        # rpm cli installed
        parent_layer = layer + "[build_appliance]",
        flavor = layer + "[flavor]",
        features = [
            feature.ensure_dirs_exist(dirs = "/layer"),
            feature.layer_mount(
                source = layer,
                mountpoint = "/layer",
            ),
        ],
    )
    image_sh_test(
        name = name,
        test = ":{}--script".format(name),
        layer = ":{}--layer".format(name),
        **kwargs
    )
