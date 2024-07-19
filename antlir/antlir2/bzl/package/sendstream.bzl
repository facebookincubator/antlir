# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:expect.bzl", "expect", "expect_non_none")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/package:cfg.bzl", "layer_attrs", "package_cfg")
load(":macro.bzl", "package_macro")

SendstreamInfo = provider(fields = [
    "sendstream",  # 'artifact' that is the btrfs sendstream
    "subvol_symlink",  # subvol that was actually packaged
    "layer",  # layer that was packaged
])

_base_sendstream_args_defaults = {
    "volume_name": "volume",
}

_base_sendstream_args = {
    "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
    "incremental_parent": attrs.option(
        attrs.dep(
            providers = [SendstreamInfo],
            doc = "create an incremental sendstream using this parent layer",
        ),
        default = None,
    ),
    "labels": attrs.list(attrs.string(), default = []),
    "layer": attrs.dep(providers = [LayerInfo]),
    "volume_name": attrs.string(default = _base_sendstream_args_defaults["volume_name"]),
}

def _sendstream_package_macro(rule):
    return package_macro(rule, always_needs_root = True)

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
    subvol_symlink = ctx.actions.declare_output("subvol_symlink")

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

    spec = ctx.actions.write_json(
        "spec.json",
        {"sendstream": {
            "incremental_parent": ctx.attrs.incremental_parent[SendstreamInfo].subvol_symlink if ctx.attrs.incremental_parent else None,
            "subvol_symlink": subvol_symlink.as_output(),
            "volume_name": ctx.attrs.volume_name,
        }},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            "sudo",
            ctx.attrs.antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(ctx.attrs.layer[LayerInfo].contents.subvol_symlink, format = "--layer={}"),
            cmd_args(sendstream.as_output(), format = "--out={}"),
        ),
        local_only = True,  # needs root and local subvol
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = True,
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

_sendstream = anon_rule(
    impl = _impl,
    artifact_promise_mappings = {
        "anon_v1_sendstream": lambda x: x[SendstreamInfo].sendstream,
    },
    attrs = _base_sendstream_args | layer_attrs,
)

def anon_v1_sendstream(
        *,
        ctx: AnalysisContext,
        layer: Dependency | None = None) -> AnonTarget:
    attrs = {
        key: getattr(ctx.attrs, key, _base_sendstream_args_defaults.get(key, None))
        for key in _base_sendstream_args
    }
    if layer:
        attrs["layer"] = layer
    attrs["name"] = str(attrs["layer"].label.raw_target()) + ".sendstream"
    return ctx.actions.anon_target(
        _sendstream,
        attrs,
    )

def _v2_impl(ctx: AnalysisContext) -> Promise:
    sendstream_v2 = ctx.actions.declare_output("image.sendstream")

    def _map(providers: ProviderCollection) -> list[Provider]:
        v1_sendstream = providers[SendstreamInfo].sendstream

        ctx.actions.run(
            cmd_args(
                ctx.attrs.sendstream_upgrader[RunInfo],
                cmd_args(str(ctx.attrs.compression_level), format = "--compression-level={}"),
                cmd_args(v1_sendstream, format = "--input={}"),
                cmd_args(sendstream_v2.as_output(), format = "--output={}"),
            ),
            category = "antlir2_package",
            identifier = "sendstream_upgrade",
            # This _can_ run remotely, but we've just produced the (often quite
            # large) sendstream artifact on this host, and we only care about the
            # final result of this v2 sendstream, so we should prefer to run locally
            # to avoid uploading and downloading many gigabytes of artifacts
            prefer_local = True,
        )
        return [
            DefaultInfo(
                sendstream_v2,
                sub_targets = {"layer": ctx.attrs.layer.providers},
            ),
            SendstreamInfo(
                sendstream = sendstream_v2,
                subvol_symlink = providers[SendstreamInfo].subvol_symlink,
                layer = providers[SendstreamInfo].layer,
            ),
        ]

    return anon_v1_sendstream(ctx = ctx).promise.map(_map)

_sendstream_v2 = rule(
    impl = _v2_impl,
    attrs = {
        "compression_level": attrs.int(default = 3),
        "sendstream_upgrader": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/btrfs_send_stream_upgrade:btrfs_send_stream_upgrade")),
    } | _base_sendstream_args | layer_attrs,
    cfg = package_cfg,
)

sendstream_v2 = _sendstream_package_macro(_sendstream_v2)
