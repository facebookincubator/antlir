# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")
load(":install.bzl", "install_rule")

def tarball(
        *,
        src: str,
        into_dir: str,
        user: str = "root",
        group: str = "root") -> ParseTimeFeature:
    return ParseTimeFeature(
        feature_type = "tarball",
        plugin = antlir2_dep("features:install"),
        srcs = {
            "src": src,
        },
        kwargs = {
            "group": group,
            "into_dir": into_dir,
            "user": user,
        },
    )

def _impl(ctx: AnalysisContext) -> Promise:
    tarball = ctx.attrs.src

    if ctx.attrs.user != "root" or ctx.attrs.group != "root":
        fail("tarball must be installed root:root")

    extracted = ctx.actions.anon_target(extract_tarball, {
        "archive": tarball,
        "name": "archive//:" + tarball.short_path,
    }).artifact("extracted")

    def _map(install_feature: ProviderCollection) -> list[Provider]:
        return [
            install_feature[DefaultInfo],
            install_feature[FeatureAnalysis],
        ]

    return ctx.actions.anon_target(install_rule, {
        "dst": ctx.attrs.into_dir + "/",
        "group": ctx.attrs.group,
        "mode": 0o755,
        "plugin": ctx.attrs.plugin,
        "src": extracted,
        "user": ctx.attrs.user,
    }).promise.map(_map)

tarball_rule = rule(
    impl = _impl,
    attrs = {
        "group": attrs.string(),
        "into_dir": attrs.string(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "src": attrs.source(),
        "user": attrs.string(),
    },
)

def _extract_impl(ctx: AnalysisContext) -> list[Provider]:
    output = ctx.actions.declare_output("extracted")

    script, _ = ctx.actions.write(
        "unpack.sh",
        [
            cmd_args(output, format = "mkdir -p {}"),
            cmd_args(output, format = "cd {}"),
            cmd_args(
                "tar",
                cmd_args("--use-compress-program=zstd") if ctx.attrs.archive.extension == ".zst" else cmd_args(),
                "-xf",
                ctx.attrs.archive,
                delimiter = " \\\n",
            ).relative_to(output),
            "\n",
        ],
        is_executable = True,
        allow_args = True,
    )
    ctx.actions.run(
        cmd_args(["/bin/sh", script])
            .hidden([ctx.attrs.archive, output.as_output()]),
        category = "extract_archive",
        local_only = True,  # needs 'zstd' binary available, also gets installed
        # into a local subvol anyway
    )

    return [
        DefaultInfo(output),
    ]

extract_tarball = anon_rule(
    impl = _extract_impl,
    attrs = {
        "archive": attrs.source(),
    },
    artifact_promise_mappings = {
        "extracted": lambda x: ensure_single_output(x[DefaultInfo]),
    },
)
