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

load("//antlir/antlir2/bzl:binaries_require_repo.bzl", "binaries_require_repo")
load("//antlir/antlir2/bzl:debuginfo.bzl", "split_binary_anon")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
    "new_feature_rule",
)
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:internal_external.bzl", "internal_external")

_EXTRACT_PLUGIN = "antlir//antlir/antlir2/features/extract:extract"
_EXTRACT_ANALYZE = "antlir//antlir/antlir2/features/extract:extract-analyze"

def extract_from_layer(
    layer: str | Select,
    binaries: list[str | Select] | Select,
    dlopen_min_priority: str | Select = "recommended",
    dlopen_features_allow: dict[str, list[str]] | Select = {},
    dlopen_features_deny: dict[str, list[str]] | Select = {},
):
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
        dlopen_min_priority: minimum priority for .note.dlopen libs to extract.
            One of "required", "recommended", "suggested". Defaults to "recommended".
        dlopen_features_allow: dict of regex -> list of features to allow.
            Regex is matched against the file path of the binary that declares the dlopen dep.
            If a dep's feature matches an allowed feature for its binary, it is included
            regardless of priority (union with priority filter).
        dlopen_features_deny: dict of regex -> list of features to deny.
            If a dep's feature matches a denied feature, it is excluded even if its
            priority would otherwise include it. Deny takes precedence over allow and priority.
    """
    return ParseTimeFeature(
        feature_type = "extract_from_layer",
        plugin = _EXTRACT_PLUGIN,
        deps = {
            "layer": layer,
        },
        exec_deps = {
            "_analyze": _EXTRACT_ANALYZE,
        },
        kwargs = {
            "binaries": binaries,
            "dlopen_features_allow": dlopen_features_allow,
            "dlopen_features_deny": dlopen_features_deny,
            "dlopen_min_priority": dlopen_min_priority,
            "target_arch": arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        },
    )

def extract_buck_binary(
    src: str | Select,
    dst: str | Select,
    strip: bool | Select = True,
    dlopen_min_priority: str | Select = "recommended",
    dlopen_features_allow: dict[str, list[str]] | Select = {},
    dlopen_features_deny: dict[str, list[str]] | Select = {},
):
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
        dlopen_min_priority: minimum priority for .note.dlopen libs to extract.
            One of "required", "recommended", "suggested". Defaults to "recommended".
        dlopen_features_allow: dict of regex -> list of features to allow.
            Regex is matched against the file path of the binary that declares the dlopen dep.
        dlopen_features_deny: dict of regex -> list of features to deny.
    """
    return ParseTimeFeature(
        feature_type = "extract_buck_binary",
        plugin = _EXTRACT_PLUGIN,
        # include in deps so we can look at the providers
        deps = {
            "src": src,
        },
        exec_deps = {
            "_analyze": _EXTRACT_ANALYZE,
            "_debuginfo_splitter": "fbcode//antlir/antlir2/tools:debuginfo-splitter",
            "_objcopy": internal_external(
                fb = "fbsource//third-party/binutils:objcopy",
                oss = "toolchains//:objcopy",
            ),
        },
        kwargs = {
            "dlopen_features_allow": dlopen_features_allow,
            "dlopen_features_deny": dlopen_features_deny,
            "dlopen_min_priority": dlopen_min_priority,
            "dst": dst,
            "strip": strip,
            "target_arch": arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        },
    )

def _extract_from_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    layer_subvol = ctx.attrs.layer[LayerInfo].contents.subvol_symlink

    manifest = ctx.actions.declare_output("manifest.json")
    libs_dir = ctx.actions.declare_output("libs_dir", dir = True)

    ctx.actions.run(
        cmd_args(
            ctx.attrs._analyze[RunInfo],
            "from-layer",
            cmd_args(layer_subvol, format = "--layer={}"),
            cmd_args(ctx.attrs.binaries, format = "--binary={}"),
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(ctx.attrs.dlopen_min_priority, format = "--dlopen-min-priority={}"),
            cmd_args(json.encode(list(ctx.attrs.dlopen_features_allow.items())), format = "--dlopen-features-allow={}"),
            cmd_args(json.encode(list(ctx.attrs.dlopen_features_deny.items())), format = "--dlopen-features-deny={}"),
            cmd_args(manifest.as_output(), format = "--manifest={}"),
            cmd_args(libs_dir.as_output(), format = "--libs-dir={}"),
        ),
        category = "extract_from_layer",
        local_only = True,  # needs local subvol
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "extract_from_layer",
            data = struct(
                provides = ctx.attrs.binaries,
                libs = struct(
                    manifest = manifest,
                    libs_dir = libs_dir,
                ),
            ),
            plugin = ctx.attrs.plugin,
        ),
    ]

extract_from_layer_rule = new_feature_rule(
    impl = _extract_from_layer_impl,
    attrs = {
        "binaries": attrs.list(attrs.string(), default = []),
        "dlopen_features_allow": attrs.dict(
            attrs.string(),
            attrs.list(attrs.string()),
            default = {},
        ),
        "dlopen_features_deny": attrs.dict(
            attrs.string(),
            attrs.list(attrs.string()),
            default = {},
        ),
        "dlopen_min_priority": attrs.string(default = "recommended"),
        "layer": attrs.dep(providers = [LayerInfo]),
        "target_arch": attrs.string(),
        "_analyze": attrs.exec_dep(),
    },
)

def _extract_buck_binary_impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.strip and binaries_require_repo.is_standalone(ctx.attrs.src):
        split_anon_target = split_binary_anon(
            ctx = ctx,
            src = ctx.attrs.src,
            objcopy = ctx.attrs._objcopy,
            debuginfo_splitter = ctx.attrs._debuginfo_splitter,
        )
        src = split_anon_target.artifact("src")
    else:
        src = ensure_single_output(ctx.attrs.src)

    manifest = ctx.actions.declare_output("manifest.json", has_content_based_path = False)
    libs_dir = ctx.actions.declare_output("libs_dir", dir = True, has_content_based_path = False)

    ctx.actions.run(
        cmd_args(
            ctx.attrs._analyze[RunInfo],
            "buck-binary",
            cmd_args(src, format = "--src={}"),
            cmd_args(ctx.attrs.dst, format = "--dst={}"),
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(ctx.attrs.dlopen_min_priority, format = "--dlopen-min-priority={}"),
            cmd_args(json.encode(list(ctx.attrs.dlopen_features_allow.items())), format = "--dlopen-features-allow={}"),
            cmd_args(json.encode(list(ctx.attrs.dlopen_features_deny.items())), format = "--dlopen-features-deny={}"),
            cmd_args(manifest.as_output(), format = "--manifest={}"),
            cmd_args(libs_dir.as_output(), format = "--libs-dir={}"),
            hidden = ctx.attrs.src[RunInfo],
        ),
        category = "extract_buck_binary",
        # The analyzer resolves shared libraries by reading the target arch's
        # fbcode platform directory off the filesystem, and RE workers only have
        # the native arch's platform installed.
        local_only = ctx.attrs.target_arch == "aarch64",
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "extract_buck_binary",
            data = struct(
                provides = [ctx.attrs.dst],
                libs = struct(
                    manifest = manifest,
                    libs_dir = libs_dir,
                ),
            ),
            plugin = ctx.attrs.plugin,
        ),
    ]

extract_buck_binary_rule = new_feature_rule(
    impl = _extract_buck_binary_impl,
    attrs = {
        "dlopen_features_allow": attrs.dict(
            attrs.string(),
            attrs.list(attrs.string()),
            default = {},
        ),
        "dlopen_features_deny": attrs.dict(
            attrs.string(),
            attrs.list(attrs.string()),
            default = {},
        ),
        "dlopen_min_priority": attrs.string(default = "recommended"),
        "dst": attrs.option(attrs.string(), default = None),
        "src": attrs.dep(providers = [RunInfo]),
        "strip": attrs.bool(default = True),
        "target_arch": attrs.string(),
        "_analyze": attrs.exec_dep(),
        "_debuginfo_splitter": attrs.option(attrs.exec_dep(), default = None),
        "_objcopy": attrs.option(attrs.exec_dep(), default = None),
    },
)
