# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")
load(":install.bzl", "install_record")

def tarball(
        *,
        src: str,
        into_dir: str,
        user: str = "root",
        group: str = "root") -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "tarball",
        impl = antlir2_dep("features:install"),
        srcs = {
            "source": src,
        },
        kwargs = {
            "group": group,
            "into_dir": into_dir,
            "user": user,
        },
        analyze_uses_context = True,
    )

tarball_record = record(
    src = Artifact,
    into_dir = str,
    force_root_ownership = bool,
)

def tarball_analyze(
        ctx: "AnalyzeFeatureContext",
        into_dir: str,
        user: str,
        group: str,
        srcs: dict[str, Artifact]) -> FeatureAnalysis.type:
    tarball = srcs["source"]

    if user != "root" or group != "root":
        fail("tarball must be installed root:root")

    extracted_anon_target = ctx.actions.anon_target(extract_tarball, {
        "archive": tarball,
        "name": "archive//:" + tarball.short_path,
    })
    extracted = ctx.actions.artifact_promise(extracted_anon_target.map(lambda x: ensure_single_output(x[DefaultInfo])))
    return FeatureAnalysis(
        data = install_record(
            src = extracted,
            dst = into_dir + "/",
            mode = 0o755,
            user = user,
            group = group,
        ),
        feature_type = "install",
        required_artifacts = [extracted],
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

extract_tarball = rule(
    impl = _extract_impl,
    attrs = {
        "archive": attrs.source(),
    },
)
