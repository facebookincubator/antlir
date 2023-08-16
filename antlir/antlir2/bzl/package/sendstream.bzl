# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

SendstreamInfo = provider(fields = {
    "sendstream": "'artifact' that is the btrfs sendstream",
})

def _impl(ctx: AnalysisContext) -> list[Provider]:
    sendstream = ctx.actions.declare_output("image.sendstream")

    spec = ctx.actions.write_json(
        "spec.json",
        {"sendstream": {
            "build_appliance": (ctx.attrs.build_appliance or ctx.attrs.layer[LayerInfo].build_appliance)[LayerInfo].subvol_symlink,
            "layer": ctx.attrs.layer[LayerInfo].subvol_symlink,
        }},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(sendstream.as_output(), format = "--out={}"),
        ),
        local_only = True,  # needs root and local subvol
        category = "antlir2_package",
    )
    return [DefaultInfo(sendstream), SendstreamInfo(sendstream = sendstream)]

_sendstream = rule(
    impl = _impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "layer": attrs.dep(providers = [LayerInfo]),
    },
)

sendstream = rule_with_default_target_platform(_sendstream)

def anon_v1_sendstream(
        *,
        ctx: AnalysisContext,
        layer: Dependency,
        build_appliance: Dependency | None = None,
        antlir2_packager: Dependency | None = None) -> "promise_artifact":
    v1_anon_target = ctx.actions.anon_target(_sendstream, {
        "antlir2_packager": antlir2_packager or ctx.attrs.antlir2_packager,
        "build_appliance": build_appliance or ctx.attrs.build_appliance,
        "layer": layer,
        "name": str(layer.label.raw_target()) + ".sendstream",
    })
    return ctx.actions.artifact_promise(
        v1_anon_target.map(lambda x: x[SendstreamInfo].sendstream),
    )

def _zst_impl(ctx: AnalysisContext) -> list[Provider]:
    sendstream_zst = ctx.actions.declare_output("image.sendstream.zst")

    v1_sendstream = anon_v1_sendstream(
        ctx = ctx,
        layer = ctx.attrs.layer,
        build_appliance = ctx.attrs.build_appliance,
    )

    ctx.actions.run(
        cmd_args(
            "zstd",
            "--compress",
            cmd_args(str(ctx.attrs.compression_level), format = "-{}"),
            "-o",
            sendstream_zst.as_output(),
            v1_sendstream,
        ),
        category = "compress",
        local_only = True,  # zstd not available on RE
    )

    return [DefaultInfo(sendstream_zst)]

_sendstream_zst = rule(
    impl = _zst_impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "compression_level": attrs.int(default = 3),
        "layer": attrs.dep(providers = [LayerInfo]),
        "sendstream_upgrader": attrs.default_only(attrs.exec_dep(default = "//antlir/btrfs_send_stream_upgrade:btrfs_send_stream_upgrade")),
    },
)

sendstream_zst = rule_with_default_target_platform(_sendstream_zst)

def _v2_impl(ctx: AnalysisContext) -> list[Provider]:
    sendstream_v2 = ctx.actions.declare_output("image.sendstream.v2")

    v1_sendstream = anon_v1_sendstream(
        ctx = ctx,
        layer = ctx.attrs.layer,
        build_appliance = ctx.attrs.build_appliance,
    )

    ctx.actions.run(
        cmd_args(
            ctx.attrs.sendstream_upgrader[RunInfo],
            cmd_args(str(ctx.attrs.compression_level), format = "--compression-level={}"),
            cmd_args(v1_sendstream, format = "--input={}"),
            cmd_args(sendstream_v2.as_output(), format = "--output={}"),
        ),
        category = "sendstream_upgrade",
        # This _can_ run remotely, but we've just produced the (often quite
        # large) sendstream artifact on this host, and we only care about the
        # final result of this v2 sendstream, so we should prefer to run locally
        # to avoid uploading and downloading many gigabytes of artifacts
        prefer_local = True,
    )

    return [DefaultInfo(sendstream_v2)]

_sendstream_v2 = rule(
    impl = _v2_impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "compression_level": attrs.int(default = 3),
        "layer": attrs.dep(providers = [LayerInfo]),
        "sendstream_upgrader": attrs.default_only(attrs.exec_dep(default = "//antlir/btrfs_send_stream_upgrade:btrfs_send_stream_upgrade")),
    },
)

sendstream_v2 = rule_with_default_target_platform(_sendstream_v2)
