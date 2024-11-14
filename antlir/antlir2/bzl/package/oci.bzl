# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":defs.bzl", "common_attrs", "default_attrs")
load(":macro.bzl", "package_macro")

OciLayer = record(
    tar = Artifact,
    tar_zst = Artifact,
)

OciLayersInfo = provider(fields = [
    "layers",  # [(BuildPhase, Artifact)]
])

def _oci_layers_impl(ctx: AnalysisContext) -> list[Provider]:
    layer = ctx.attrs.layer[LayerInfo]

    oci_layers = []
    layers = list(layer.phase_contents)
    if layer.parent:
        layers.insert(0, (None, layer.parent[LayerInfo].contents))
    else:
        layers.insert(0, None)
    for parent, (child_phase, child_contents) in zip(layers, layers[1:]):
        tar = ctx.actions.declare_output(child_phase.value, "layer.tar")
        if parent:
            parent = parent[1]  # parent phase info doesn't matter, throw it away
        ctx.actions.run(
            cmd_args(
                "sudo" if not ctx.attrs._rootless else cmd_args(),
                ctx.attrs._make_oci_layer[RunInfo],
                "--rootless" if ctx.attrs._rootless else cmd_args(),
                cmd_args(parent.subvol_symlink, format = "--parent={}") if parent else cmd_args(),
                cmd_args(child_contents.subvol_symlink, format = "--child={}"),
                cmd_args(tar.as_output(), format = "--out={}"),
            ),
            local_only = True,  # comparing local subvols
            category = "oci_layer",
            identifier = child_phase.value,
        )

        # the uncompressed tar is needed for hashing, but then we want to put a
        # compressed tar in the actual archive
        # need a compressed tar to actually put in the archive, but the
        tar_zst = ctx.actions.declare_output(child_phase.value, "layer.tar.zst")
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
            identifier = child_phase.value,
        )
        oci_layers.append((child_phase, OciLayer(
            tar = tar,
            tar_zst = tar_zst,
        )))

    return [
        DefaultInfo(),
        OciLayersInfo(layers = oci_layers),
    ]

_oci_layers = anon_rule(
    impl = _oci_layers_impl,
    attrs = {
        "layer": attrs.dep(providers = [LayerInfo]),
        "_make_oci_layer": attrs.default_only(
            attrs.exec_dep(
                default = "antlir//antlir/antlir2/antlir2_packager/make_oci_layer:make-oci-layer",
            ),
        ),
        "_rootless": attrs.bool(),
    },
    artifact_promise_mappings = {},
)

def _impl(ctx: AnalysisContext) -> Promise:
    layers = [ctx.attrs.layer]
    for _ in range(0, 1000):
        if not layers[0][LayerInfo].parent:
            break
        layers.insert(0, layers[0][LayerInfo].parent)

    def _with_anon(oci_multi_layers) -> list[Provider]:
        deltas = []
        sub_targets_layers = {}

        for i, multi_layer in enumerate(oci_multi_layers):
            multi_layer_subtargets = {}
            for phase, layer in multi_layer[OciLayersInfo].layers:
                deltas.append(layer)
                multi_layer_subtargets[phase.value] = [DefaultInfo(sub_targets = {
                    "tar": [DefaultInfo(layer.tar)],
                    "tar.zst": [DefaultInfo(layer.tar_zst)],
                })]
            sub_targets_layers[str(i)] = [DefaultInfo(sub_targets = multi_layer_subtargets)]

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

    return ctx.actions.anon_targets([
        (
            _oci_layers,
            {
                "layer": layer,
                "name": layer[LayerInfo].label,
                "_make_oci_layer": ctx.attrs._make_oci_layer,
                "_rootless": ctx.attrs._rootless,
            },
        )
        for layer in layers
    ]).promise.map(_with_anon)

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
