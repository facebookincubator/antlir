# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
WARNING: you probably don't actually want this
extract.bzl exists for very stripped down environments (for example, building
an initrd) that need a binary (most likely from an RPM) and its library
dependencies. In almost every case _other_ than building an initrd, you
either want `feature.rpms_install` or `feature.install_buck_runnable`

If you're still here, `extract.extract` works by parsing the ELF information
in the given binaries.
It then clones the binaries and any .so's they depend on from the source
layer into the destination layer. The actual clone is very unergonomic at
this point, and it is recommended to batch all binaries to be extracted into
a single call to `extract.extract`.

This new-and-improved version of extract is capable of extracting buck-built
binaries without first installing them into a layer.
"""

load("//antlir/antlir2/bzl:debuginfo.bzl", "split_binary_anon")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def extract_from_layer(
        layer: str | Select,
        binaries: list[str | Select] | Select) -> ParseTimeFeature:
    """
    Extract binaries that are installed into `layer`, most commonly by RPMs.

    This copies the binary as well as any `.so` dependencies that `ld.so --list`
    reports. All the dependencies are copied from within `layer`. Any conflicts
    (same path, different file hash) caused by the extractor will result in a
    build error.
    """
    return ParseTimeFeature(
        feature_type = "extract",
        plugin = antlir2_dep("features:extract"),
        deps = {
            "layer": layer,
        },
        kwargs = {
            "binaries": binaries,
            "kind": "layer",
        },
    )

def _should_strip(strip_attr: bool) -> bool:
    # Only strip if both strip=True and we're in opt mode (standalone binaries)
    return strip_attr and not REPO_CFG.artifacts_require_repo

def extract_buck_binary(
        src: str | Select,
        dst: str | Select,
        strip: bool | Select = True) -> ParseTimeFeature:
    """
    Extract a binary built by buck into the target layer.

    The `.so` dependencies in this case will be copied from the host filesystem,
    but the same conflict detection method as `extract_from_layer` is employed.
    """
    return ParseTimeFeature(
        feature_type = "extract",
        plugin = antlir2_dep("features:extract"),
        # include in deps so we can look at the providers
        deps = {
            "src": src,
        },
        exec_deps = {
            "_objcopy": "fbsource//third-party/binutils:objcopy",
        } if _should_strip(strip) else {},
        kwargs = {
            "dst": dst,
            "kind": "buck",
            "strip": strip,
        },
    )

extract_buck_record = record(
    src = Artifact,
    dst = str,
)

extract_layer_record = record(
    layer = layer_dep,
    binaries = list[str],
)

extract_record = record(
    buck = [extract_buck_record, None],
    layer = [extract_layer_record, None],
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.kind == "layer":
        return [
            DefaultInfo(),
            FeatureAnalysis(
                feature_type = "extract",
                data = extract_record(
                    layer = extract_layer_record(
                        layer = layer_dep_analyze(ctx.attrs.layer),
                        binaries = ctx.attrs.binaries,
                    ),
                    buck = None,
                ),
                required_layers = [ctx.attrs.layer[LayerInfo]],
                plugin = ctx.attrs.plugin[FeaturePluginInfo],
            ),
        ]
    elif ctx.attrs.kind == "buck":
        src_runinfo = ctx.attrs.src[RunInfo]

        if _should_strip(ctx.attrs.strip):
            split_anon_target = split_binary_anon(
                ctx = ctx,
                src = ctx.attrs.src,
                objcopy = ctx.attrs._objcopy,
            )
            src = split_anon_target.artifact("src")
        else:
            src = ensure_single_output(ctx.attrs.src)

        return [
            DefaultInfo(),
            FeatureAnalysis(
                feature_type = "extract",
                data = extract_record(
                    buck = extract_buck_record(
                        src = src,
                        dst = ctx.attrs.dst,
                    ),
                    layer = None,
                ),
                required_artifacts = [src],
                required_run_infos = [src_runinfo],
                plugin = ctx.attrs.plugin[FeaturePluginInfo],
            ),
        ]
    else:
        fail("invalid extract kind '{}'".format(ctx.attrs.kind))

extract_rule = rule(
    impl = _impl,
    attrs = {
        "binaries": attrs.list(attrs.string(), default = []),
        "dst": attrs.option(attrs.string(), default = None),
        "kind": attrs.enum(["layer", "buck"]),
        "layer": attrs.option(
            attrs.dep(providers = [LayerInfo]),
            default = None,
        ),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "src": attrs.option(
            attrs.dep(providers = [RunInfo]),
            default = None,
        ),
        "strip": attrs.bool(default = True),
        "_objcopy": attrs.option(attrs.exec_dep(), default = None),
    },
)
