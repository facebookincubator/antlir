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
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:dependency_layer_info.bzl", "layer_dep_analyze")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:build_defs.bzl", "internal_external")
load("//antlir/bzl:constants.bzl", "REPO_CFG")

def extract_from_layer(
        layer: str | Select,
        binaries: list[str | Select] | Select):
    """
    Extract a binary and all of its runtime dependencies from `layer` into the
    target layer.

    This copies the binary and all of it's `.so` dependencies from the host
    filesystem. Any mismatched contents in these dependencies will cause an
    image build failure.

    :::warning You almost definitely **do NOT** want this

    This feature exists only for building *extremely* stripped down environments
    like initrds, where things like the fbcode runtime is unavailable.

    In 99% of cases you actually just want to use
    [`feature.install`](#featureinstall) or
    [`feature.rpms_install`](#featurerpms_install)
    :::

    Arguments:
        layer: antlir2 layer target to extract from
        binaries: list of file paths to extract
    """
    return ParseTimeFeature(
        feature_type = "extract_from_layer",
        plugin = "antlir//antlir/antlir2/features/extract:extract_from_layer",
        antlir2_configured_deps = {
            "layer": layer,
        },
        kwargs = {
            "binaries": binaries,
        },
    )

def _should_strip(strip_attr: bool) -> bool:
    # Only strip if both strip=True and we're in opt mode (standalone binaries)
    return strip_attr and not REPO_CFG.artifacts_require_repo

def extract_buck_binary(
        src: str | Select,
        dst: str | Select,
        strip: bool | Select = True):
    """
    Extract a buck-built binary and all of its runtime dependencies into the
    target layer.

    This copies the binary and all of it's `.so` dependencies from the host
    filesystem. Any mismatched contents in these dependencies will cause an
    image build failure.

    :::warning You almost definitely **do NOT** want this

    This feature exists only for building *extremely* stripped down environments
    like initrds, where things like the fbcode runtime is unavailable.

    In 99% of cases you actually want to just give your binary to
    [`feature.install`](#featureinstall)
    :::

    Arguments:
        src: binary target
        dst: path to install it to in the image
        strip: strip debug info from the binary and discard it
    """
    return ParseTimeFeature(
        feature_type = "extract_buck_binary",
        plugin = "antlir//antlir/antlir2/features/extract:extract_buck_binary",
        # include in deps so we can look at the providers
        deps = {
            "src": src,
        },
        exec_deps = {
            "_analyze": "antlir//antlir/antlir2/features/extract:extract-buck-binary-analyze",
        } | (
            {
                "_objcopy": internal_external(
                    fb = "fbsource//third-party/binutils:objcopy",
                    oss = "toolchains//:objcopy",
                ),
            } if _should_strip(strip) else {}
        ),
        kwargs = {
            "dst": dst,
            "strip": strip,
            "target_arch": arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        },
    )

def _extract_from_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "extract_from_layer",
            data = struct(
                layer = layer_dep_analyze(ctx.attrs.layer),
                binaries = ctx.attrs.binaries,
            ),
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

extract_from_layer_rule = rule(
    impl = _extract_from_layer_impl,
    attrs = {
        "binaries": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
    },
)

def _extract_buck_binary_impl(ctx: AnalysisContext) -> list[Provider]:
    if _should_strip(ctx.attrs.strip):
        split_anon_target = split_binary_anon(
            ctx = ctx,
            src = ctx.attrs.src,
            objcopy = ctx.attrs._objcopy,
        )
        src = split_anon_target.artifact("src")
    else:
        src = ensure_single_output(ctx.attrs.src)

    manifest = ctx.actions.declare_output("manifest.json")
    libs_dir = ctx.actions.declare_output("libs_dir", dir = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._analyze[RunInfo],
            cmd_args(src, format = "--src={}"),
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(manifest.as_output(), format = "--manifest={}"),
            cmd_args(libs_dir.as_output(), format = "--libs-dir={}"),
            hidden = ctx.attrs.src[RunInfo],
        ),
        category = "extract_buck_binary",
        # RE seems to not have aarch64 cross stuff set up, so for now just force
        # aarch64 extracts to run locally
        local_only = ctx.attrs.target_arch == "aarch64",
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "extract_buck_binary",
            data = struct(
                src = src,
                dst = ctx.attrs.dst,
                libs = struct(
                    manifest = manifest,
                    libs_dir = libs_dir,
                ),
            ),
            required_artifacts = [src],
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

extract_buck_binary_rule = rule(
    impl = _extract_buck_binary_impl,
    attrs = {
        "dst": attrs.option(attrs.string(), default = None),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "src": attrs.dep(providers = [RunInfo]),
        "strip": attrs.bool(default = True),
        "target_arch": attrs.string(),
        "_analyze": attrs.exec_dep(),
        "_objcopy": attrs.option(attrs.exec_dep(), default = None),
    },
)
