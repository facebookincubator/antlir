# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":defs.bzl", "common_attrs", "default_attrs")
load(":macro.bzl", "package_macro")

OciLayerInfo = provider(fields = {
    "tar": Artifact,
    "tar_zst": Artifact,
})

def _oci_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    layer = ctx.attrs.child[LayerInfo]
    tar = ctx.actions.declare_output("layer.tar")
    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs._make_oci_layer[RunInfo],
            "--rootless" if ctx.attrs._rootless else cmd_args(),
            cmd_args(layer.parent[LayerInfo].subvol_symlink, format = "--parent={}") if layer.parent else cmd_args(),
            cmd_args(layer.subvol_symlink, format = "--child={}"),
            cmd_args(tar.as_output(), format = "--out={}"),
        ),
        local_only = True,  # comparing local subvols
        category = "oci_layer",
    )

    # the uncompressed tar is needed for hashing, but then we want to put a
    # compressed tar in the actual archive
    # need a compressed tar to actually put in the archive, but the
    tar_zst = ctx.actions.declare_output("layer.tar.zst")
    ctx.actions.run(
        cmd_args(
            "zstd",
            "--compress",
            "-15",
            "-T0",  # we like threads
            tar,
            "-o",
            tar_zst.as_output(),
        ),
        category = "oci_layer_compress",
    )
    return [
        DefaultInfo(),
        OciLayerInfo(
            tar = tar,
            tar_zst = tar_zst,
        ),
    ]

_oci_layer = anon_rule(
    impl = _oci_layer_impl,
    attrs = {
        "child": attrs.dep(providers = [LayerInfo]),
        "_make_oci_layer": attrs.default_only(
            attrs.exec_dep(
                default = "antlir//antlir/antlir2/antlir2_packager/make_oci_layer:make-oci-layer",
            ),
        ),
        "_rootless": attrs.bool(),
    },
    artifact_promise_mappings = {
        "tar": lambda x: x[OciLayerInfo].tar,
        "tar_zst": lambda x: x[OciLayerInfo].tar_zst,
    },
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    layers = [ctx.attrs.layer]
    for _ in range(0, 1000):
        if not layers[0][LayerInfo].parent:
            break
        layers.insert(0, layers[0][LayerInfo].parent)

    deltas = []
    sub_targets_layers = {}

    for i, child in enumerate(layers):
        anon_target = ctx.actions.anon_target(
            _oci_layer,
            {
                "child": child,
                "name": child[LayerInfo].label,
                "_make_oci_layer": ctx.attrs._make_oci_layer,
                "_rootless": ctx.attrs._rootless,
            },
        )

        tar = anon_target.artifact("tar")
        tar_zst = anon_target.artifact("tar_zst")

        deltas.append(struct(
            tar = tar,
            tar_zst = tar_zst,
        ))
        sub_targets_layers[str(i)] = [DefaultInfo(sub_targets = {
            "tar": [DefaultInfo(tar)],
            "tar.zst": [DefaultInfo(tar_zst)],
        })]

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
        DefaultInfo(
            out,
            sub_targets = {"layers": [DefaultInfo(sub_targets = sub_targets_layers)]},
        ),
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
