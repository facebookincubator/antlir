# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:toolchain.bzl", "Antlir2ToolchainInfo")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":depgraph.bzl", "build_depgraph")

PrebuiltImageInfo = provider(fields = {
    "format": "format of the image file",
    "source": "source file of the image",
})

def _impl(ctx: "context") -> ["provider"]:
    subvol_symlink = ctx.actions.declare_output("subvol_symlink")
    ctx.actions.run(
        cmd_args(
            "sudo",  # this requires privileged btrfs operations
            ctx.attrs.antlir2_receive[RunInfo],
            "--working-dir=antlir2-out",
            cmd_args(str(ctx.label), format = "--label={}"),
            cmd_args(ctx.attrs.format, format = "--format={}"),
            cmd_args(ctx.attrs.src, format = "--source={}"),
            cmd_args(subvol_symlink.as_output(), format = "--output={}"),
        ),
        category = "antlir2_prebuilt_layer",
        # needs local subvolumes
        local_only = True,
        # 'antlir2-receive' will clean up an old image if it exists
        no_outputs_cleanup = True,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
    )
    depgraph_output = build_depgraph(
        ctx = ctx,
        parent_depgraph = None,
        features = None,
        features_json = None,
        format = "json",
        subvol = subvol_symlink,
        dependency_layers = [],
    )
    return [
        LayerInfo(
            label = ctx.label,
            depgraph = depgraph_output,
            subvol_symlink = subvol_symlink,
            mounts = [],
        ),
        DefaultInfo(subvol_symlink),
    ]

prebuilt = rule(
    impl = _impl,
    attrs = {
        # It's still worth splitting out toolchain and antlir2_receive since
        # only the post-processed depgraph will be invalidated if the toolchain
        # changes, not the cached layer itself
        "antlir2_receive": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_receive:antlir2-receive")),
        "format": attrs.enum(["sendstream.v2"]),
        "src": attrs.source(doc = "source file of the image"),
        "toolchain": attrs.toolchain_dep(
            providers = [Antlir2ToolchainInfo],
            default = "//antlir/antlir2:toolchain",
        ),
    },
)
