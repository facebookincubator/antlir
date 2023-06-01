# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")

def _rpm_names_test_impl(ctx: "context") -> ["provider"]:
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            ctx.attrs.image_rpms_test[RunInfo],
            "names",
            ctx.attrs.src,
            "--",
            "--layer=/layer",
        )),
    ]

_rpm_names_test = rule(
    impl = _rpm_names_test_impl,
    attrs = {
        "image_rpms_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_rpms_test:image-rpms-test")),
        "src": attrs.source(),
    },
)

def image_test_rpm_names(name: str.type, src: str.type, layer: str.type, **kwargs):
    _rpm_names_test(
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

def _rpm_integrity_test_impl(ctx: "context") -> ["provider"]:
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            ctx.attrs.image_rpms_test[RunInfo],
            "integrity",
            # image_test generally does not add this because it needs to parse
            # options intended for the end test in the case of things like
            # python_unittest. Add -- explicitly so that `image-test` does not
            # try to parse our rpm test options
            "--",
            cmd_args(ctx.attrs.ignored_files, format = "--ignored-file={}"),
            cmd_args(ctx.attrs.ignored_rpms, format = "--ignored-rpm={}"),
            "--layer=/layer",
        )),
    ]

_rpm_integrity_test = rule(
    impl = _rpm_integrity_test_impl,
    attrs = {
        "ignored_files": attrs.list(attrs.string(doc = "path that is allowed to fail integrity test"), default = []),
        "ignored_rpms": attrs.list(attrs.string(doc = "name of rpm that is ignored for integrity checks"), default = []),
        "image_rpms_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_rpms_test:image-rpms-test")),
    },
)

def image_test_rpm_integrity(
        name: str.type,
        layer: str.type,
        ignored_files: [str.type] = [],
        ignored_rpms: [str.type] = [],
        **kwargs):
    """
    Verify the integrity of all installed RPMs to ensure that any changes done
    by an image will not be undone by any runtime rpm installation.
    """
    _rpm_integrity_test(
        name = name + "--script",
        ignored_files = ignored_files,
        ignored_rpms = ignored_rpms,
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
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
        **kwargs
    )
