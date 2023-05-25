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

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:types.bzl", "types")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeDependency", "ParseTimeFeature")

types.lint_noop()

def extract_from_layer(
        layer: types.or_selector(str.type),
        binaries: types.or_selector([types.or_selector(str.type)])) -> ParseTimeFeature.type:
    """
    Extract binaries that are installed into `layer`, most commonly by RPMs.

    This copies the binary as well as any `.so` dependencies that `ld.so --list`
    reports. All the dependencies are copied from within `layer`. Any conflicts
    (same path, different file hash) caused by the extractor will result in a
    build error.
    """
    return ParseTimeFeature(
        feature_type = "extract",
        deps = {
            "layer": ParseTimeDependency(dep = layer, providers = [LayerInfo]),
        },
        kwargs = {
            "binaries": binaries,
            "source": "layer",
        },
    )

def extract_buck_binary(
        src: types.or_selector(str.type),
        dst: types.or_selector(str.type)) -> ParseTimeFeature.type:
    """
    Extract a binary built by buck into the target layer.

    The `.so` dependencies in this case will be copied from the host filesystem,
    but the same conflict detection method as `extract_from_layer` is employed.
    """
    return ParseTimeFeature(
        feature_type = "extract",
        # include in deps so we can look at the providers
        deps = {"src": ParseTimeDependency(dep = src, providers = [RunInfo])},
        kwargs = {
            "dst": dst,
            "source": "buck",
        },
    )

extract_buck_record = record(
    src = "artifact",
    dst = str.type,
)

extract_layer_record = record(
    layer = layer_dep.type,
    binaries = [str.type],
)

extract_record = record(
    buck = [extract_buck_record.type, None],
    layer = [extract_layer_record.type, None],
)

def extract_analyze(
        source: str.type,
        deps: {str.type: "dependency"},
        binaries: [[str.type], None] = None,
        src: [str.type, None] = None,
        dst: [str.type, None] = None) -> FeatureAnalysis.type:
    if source == "layer":
        layer = deps["layer"]
        return FeatureAnalysis(
            data = extract_record(
                layer = extract_layer_record(
                    layer = layer_dep_analyze(layer),
                    binaries = binaries,
                ),
                buck = None,
            ),
            required_layers = [layer[LayerInfo]],
        )
    elif source == "buck":
        src = deps["src"]
        if RunInfo not in src:
            fail("'{}' does not appear to be a binary".format(src))
        return FeatureAnalysis(
            data = extract_record(
                buck = extract_buck_record(
                    src = ensure_single_output(src),
                    dst = dst,
                ),
                layer = None,
            ),
            required_artifacts = [ensure_single_output(src)],
            required_run_infos = [src[RunInfo]],
        )
    else:
        fail("invalid extract source '{}'".format(source))
