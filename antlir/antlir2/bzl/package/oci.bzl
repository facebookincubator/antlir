# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerContents", "LayerInfo")
load(":attrs.bzl", "common_attrs", "default_attrs")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":macro.bzl", "package_macro")

OciLayer = record(
    tar = Artifact,
    tar_zst = Artifact,
)

OciLayersInfo = provider(fields = [
    "layers",  # [(BuildPhase, Artifact)]
])

def _make_layer_tar(
        *,
        ctx: AnalysisContext,
        identifier: str,
        parent: LayerContents | None,
        child_subvol: LayerContents) -> OciLayer:
    tar = ctx.actions.declare_output(identifier, "layer.tar", has_content_based_path = False)
    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs._make_oci_layer[RunInfo],
            "--rootless" if ctx.attrs._rootless else cmd_args(),
            cmd_args(parent.subvol_symlink, format = "--parent={}") if parent else cmd_args(),
            cmd_args(child_subvol.subvol_symlink, format = "--child={}"),
            cmd_args(tar.as_output(), format = "--out={}"),
            cmd_args(ctx.attrs.strip_paths, format = "--strip-path={}"),
            cmd_args(ctx.attrs.retain_paths, format = "--retain-path={}"),
        ),
        local_only = True,  # comparing local subvols
        category = "oci_layer",
        identifier = identifier,
    )

    # the uncompressed tar is needed for hashing, but then we want to put a
    # compressed tar in the actual archive
    tar_zst = ctx.actions.declare_output(identifier, "layer.tar.zst", has_content_based_path = False)
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
        identifier = identifier,
    )
    return OciLayer(tar = tar, tar_zst = tar_zst)

def _oci_layers_impl(ctx: AnalysisContext) -> list[Provider]:
    layer = ctx.attrs.layer[LayerInfo]

    oci_layers = []

    if ctx.attrs.collapse_into_one_layer:
        # Produce a single layer from the final contents against empty,
        # ignoring all parent layers and phase breakdowns.
        last_phase, _last_contents = layer.phase_contents[-1]
        oci_layers.append((last_phase, _make_layer_tar(
            ctx = ctx,
            identifier = "collapsed",
            parent = None,
            child_subvol = layer.contents,
        )))
    else:
        layers = list(layer.phase_contents)
        if layer.parent:
            layers.insert(0, (None, layer.parent[LayerInfo].contents))
        else:
            layers.insert(0, None)
        for parent, (child_phase, child_contents) in zip(layers, layers[1:]):
            if parent:
                parent = parent[1]  # parent phase info doesn't matter, throw it away
            oci_layers.append((child_phase, _make_layer_tar(
                ctx = ctx,
                identifier = child_phase.value,
                parent = parent,
                child_subvol = child_contents,
            )))

    return [
        DefaultInfo(),
        OciLayersInfo(layers = oci_layers),
    ]

_oci_layers = anon_rule(
    impl = _oci_layers_impl,
    attrs = {
        "collapse_into_one_layer": attrs.bool(default = False),
        "layer": attrs.dep(providers = [LayerInfo]),
        "retain_paths": attrs.list(attrs.string(), default = []),
        "strip_paths": attrs.list(attrs.string(), default = []),
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
    if ctx.attrs.collapse_into_one_layer:
        layers = [ctx.attrs.layer]
    else:
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

        out = ctx.actions.declare_output(ctx.label.name, dir = True, has_content_based_path = False)
        spec_oci = {
            "deltas": deltas,
            "entrypoint": ctx.attrs.entrypoint,
            "facts_db": ctx.attrs.layer[LayerInfo].facts_db,
            "ref": ctx.attrs.ref,
            "skopeo": ctx.attrs._skopeo[DefaultInfo].default_outputs[0],
            "skopeo_policy": ctx.attrs._skopeo_policy[DefaultInfo].default_outputs[0],
            "target_arch": ctx.attrs._target_arch,
            "zstd_chunked": ctx.attrs.zstd_chunked,
        }
        spec = ctx.actions.write_json(
            "spec.json",
            {"oci": spec_oci},
            with_inputs = True,
        )
        ctx.actions.run(
            cmd_args(
                ctx.attrs._antlir2_packager[RunInfo],
                "--dir",
                cmd_args(out.as_output(), format = "--out={}"),
                cmd_args(spec, format = "--spec={}"),
                cmd_args(ctx.attrs._working_format, format = "--working-format={}"),
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
                "collapse_into_one_layer": ctx.attrs.collapse_into_one_layer,
                "layer": layer,
                "name": layer[LayerInfo].label,
                "retain_paths": [str(p) for p in ctx.attrs.retain_paths],
                "strip_paths": [str(p) for p in ctx.attrs.strip_paths],
                "_make_oci_layer": ctx.attrs._make_oci_layer,
                "_rootless": ctx.attrs._rootless,
            },
        )
        for layer in layers
    ]).promise.map(_with_anon)

oci_attrs = {
    "collapse_into_one_layer": attrs.bool(default = False, doc = "If True, collapse all layers into a single layer containing the final filesystem state"),
    "entrypoint": attrs.list(attrs.string(), doc = "Command to run as the main process"),
    "ref": attrs.string(
        default = native.read_config("build_info", "revision", "local"),
        doc = "Ref name for OCI image",
    ),
    "retain_paths": attrs.list(attrs.regex(), default = [], doc = "List of regexes matched against absolute paths in the image; any path that does NOT match is excluded from the OCI layer tar"),
    "strip_paths": attrs.list(attrs.regex(), default = [], doc = "List of regexes matched against absolute paths in the image; any matching path is excluded from the OCI layer tar"),
    "zstd_chunked": attrs.bool(
        default = False,
        doc = "If True, re-materialize the OCI layout with zstd:chunked-compressed layers via skopeo",
    ),
    "_make_oci_layer": attrs.default_only(
        attrs.exec_dep(
            default = "antlir//antlir/antlir2/antlir2_packager/make_oci_layer:make-oci-layer",
        ),
    ),
    "_skopeo": attrs.default_only(
        attrs.exec_dep(
            default = "//crackerjack/vaagent:skopeo",
        ),
    ),
    "_skopeo_policy": attrs.default_only(
        attrs.exec_dep(
            default = "//crackerjack/vaagent:skopeo_policy",
        ),
    ),
}

oci_rule = rule(
    impl = _impl,
    attrs = oci_attrs | layer_attrs | default_attrs | common_attrs,
    cfg = package_cfg,
)

oci = package_macro(oci_rule)
