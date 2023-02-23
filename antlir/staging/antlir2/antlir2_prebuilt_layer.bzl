# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":antlir2_layer.bzl", "build_depgraph")
load(":antlir2_layer_info.bzl", "LayerInfo")

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
            "--working-dir=buck-image-out/volume/antlir2",
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
    depgraph_output = build_depgraph(ctx, "json", subvol_symlink, [])
    return [
        LayerInfo(
            label = ctx.label,
            depgraph = depgraph_output,
            subvol_symlink = subvol_symlink,
        ),
        DefaultInfo(subvol_symlink),
    ]

antlir2_prebuilt_layer = rule(
    impl = _impl,
    attrs = {
        # It's still worth splitting out antlir2 and antlir2_receive since only
        # the post-processed depgraph will be invalidated if antlir2 changes,
        # not the cached layer itself
        "antlir2": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/antlir2:antlir2")),
        "antlir2_receive": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/antlir2_receive:antlir2-receive")),
        "format": attrs.enum(["sendstream.v2"]),
        "src": attrs.source(doc = "source file of the image"),
    },
)
