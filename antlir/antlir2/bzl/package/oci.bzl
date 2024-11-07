# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":defs.bzl", "common_attrs", "default_attrs")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    layers = [ctx.attrs.layer[LayerInfo]]
    for _ in range(0, 1000):
        if not layers[0].parent:
            break
        layers.insert(0, layers[0].parent[LayerInfo])

    layers.insert(0, None)
    deltas = []
    for i, (first, next) in enumerate(zip(layers, layers[1:])):
        uncompressed = ctx.actions.declare_output("layer_{}.tar".format(i))
        ctx.actions.run(
            cmd_args(
                "sudo" if not ctx.attrs._rootless else cmd_args(),
                ctx.attrs._make_oci_layer[RunInfo],
                "--rootless" if ctx.attrs._rootless else cmd_args(),
                cmd_args(first.subvol_symlink, format = "--parent={}") if first else cmd_args(),
                cmd_args(next.subvol_symlink, format = "--child={}"),
                cmd_args(uncompressed.as_output(), format = "--out={}"),
            ),
            local_only = True,  # comparing local subvols
            category = "oci_layer",
            identifier = str(i),
        )

        # the uncompressed tar is needed for hashing, but then we want to put a
        # compressed tar in the actual archive
        # need a compressed tar to actually put in the archive, but the
        compressed = ctx.actions.declare_output("layer_{}.tar.zst".format(i))
        ctx.actions.run(
            cmd_args(
                "zstd",
                "--compress",
                "-15",
                "-T0",  # we like threads
                uncompressed,
                "-o",
                compressed.as_output(),
            ),
            category = "oci_layer_compress",
            identifier = str(i),
        )
        deltas.append(struct(
            tar = uncompressed,
            tar_zst = compressed,
        ))

    out = ctx.actions.declare_output(ctx.label.name, dir = True)
    spec = ctx.actions.write_json(
        "spec.json",
        {"oci": {
            "deltas": deltas,
            "entrypoint": ctx.attrs.entrypoint,
            "ref": ctx.attrs.ref,
            "target_arch": ctx.attrs._target_arch,
        }},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            "--dir",
            cmd_args(out.as_output(), format = "--out={}"),
            cmd_args(spec, format = "--spec={}"),
        ),
        category = "antlir2_package",
        identifier = "oci",
    )
    return [
        DefaultInfo(out),
        RunInfo(cmd_args(out)),
    ]

oci_attrs = {
    "entrypoint": attrs.list(attrs.string(), doc = "Command to run as the main process"),
    "ref": attrs.string(
        default = native.read_config("build_info", "revision", "local"),
        doc = "Ref name for OCI image",
    ),
    "_make_oci_layer": attrs.default_only(
        attrs.exec_dep(
            default = "antlir//antlir/antlir2/antlir2_packager/make_oci_layer:make-oci-layer",
        ),
    ),
}

oci_rule = rule(
    impl = _impl,
    attrs = oci_attrs | layer_attrs | default_attrs | common_attrs,
    cfg = package_cfg,
)

oci = package_macro(oci_rule)
