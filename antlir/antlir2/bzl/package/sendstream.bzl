# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:expect.bzl", "expect", "expect_non_none")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/package:cfg.bzl", "layer_attrs", "package_cfg")
load(":macro.bzl", "package_macro")

SendstreamInfo = provider(fields = [
    "sendstream",  # 'artifact' that is the btrfs sendstream
    "subvol_symlink",  # subvol that was actually packaged
    "layer",  # the layer this came from
])

def _find_incremental_parent(*, layer: LayerInfo, parent_label: Label) -> Dependency | None:
    if not layer.parent:
        return None
    if parent_label.raw_target() in (
        layer.parent.label.raw_target(),
        layer.parent[LayerInfo].label.raw_target(),
    ):
        return layer.parent
    return _find_incremental_parent(
        layer = layer.parent[LayerInfo],
        parent_label = parent_label,
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    sendstream = ctx.actions.declare_output("image.sendstream")

    userspace = ctx.attrs._rootless

    if not userspace:
        subvol_symlink = ctx.actions.declare_output("subvol_symlink")
    else:
        subvol_symlink = ctx.attrs.layer[LayerInfo].subvol_symlink

    if ctx.attrs.incremental_parent:
        incremental_parent_layer = _find_incremental_parent(
            layer = ctx.attrs.layer[LayerInfo],
            parent_label = ctx.attrs.incremental_parent[SendstreamInfo].layer.label,
        )
        expect_non_none(
            incremental_parent_layer,
            "{} (aka {}) is not an ancestor of {}",
            ctx.attrs.incremental_parent.label.raw_target(),
            ctx.attrs.incremental_parent[SendstreamInfo].layer.label.raw_target(),
            ctx.attrs.layer.label.raw_target(),
        )

        # this should be impossible, but let's be very careful
        expect(
            ctx.attrs.layer[LayerInfo].flavor.label.raw_target() ==
            incremental_parent_layer[LayerInfo].flavor.label.raw_target(),
            "flavor ({}) was different from incremental_parent's flavor ({})",
            ctx.attrs.layer[LayerInfo].flavor.label,
            incremental_parent_layer[LayerInfo].flavor.label,
        )

        if userspace:
            incremental_parent = ctx.attrs.incremental_parent[SendstreamInfo].layer[LayerInfo].subvol_symlink
        else:
            incremental_parent = ctx.attrs.incremental_parent[SendstreamInfo].subvol_symlink
        if incremental_parent == None:
            fail("failed to get subvol_symlink from incremental_parent, cannot proceed: {}".format(
                ctx.attrs.incremental_parent[SendstreamInfo],
            ))
    else:
        incremental_parent = None

    spec = ctx.actions.write_json(
        "spec.json",
        {"sendstream": {
            "compression_level": ctx.attrs.compression_level,
            "incremental_parent": incremental_parent,
            "subvol_symlink": subvol_symlink.as_output() if not userspace else None,
            "userspace": userspace,
            "volume_name": ctx.attrs.volume_name,
        }},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_packager[RunInfo],
            "--rootless" if ctx.attrs._rootless else cmd_args(),
            cmd_args(spec, format = "--spec={}"),
            cmd_args(ctx.attrs.layer[LayerInfo].contents.subvol_symlink, format = "--layer={}"),
            cmd_args(sendstream.as_output(), format = "--out={}"),
        ),
        local_only = True,  # needs root and local subvol
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = not userspace,
        category = "antlir2_package",
        identifier = "sendstream",
        env = {"RUST_LOG": "trace"},
    )
    return [
        DefaultInfo(sendstream),
        SendstreamInfo(
            sendstream = sendstream,
            subvol_symlink = subvol_symlink,
            layer = ctx.attrs.layer,
        ),
    ]

_attrs = {
    "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
    "compression_level": attrs.int(default = 3),
    "incremental_parent": attrs.option(
        attrs.dep(
            providers = [SendstreamInfo],
            doc = "create an incremental sendstream using this parent layer",
        ),
        default = None,
    ),
    "labels": attrs.list(attrs.string(), default = []),
    "volume_name": attrs.string(default = "volume"),
    "_rootless": rootless_cfg.is_rootless_attr,
} | layer_attrs | rootless_cfg.attrs

_sendstream_v2 = rule(
    impl = _impl,
    attrs = _attrs,
    cfg = package_cfg,
)

sendstream_v2 = package_macro(_sendstream_v2)

sendstream_v2_anon = anon_rule(
    attrs = _attrs,
    impl = _impl,
    artifact_promise_mappings = {
        "sendstream": lambda x: x[SendstreamInfo].sendstream,
    },
)
