# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")
load(":install.bzl", "install_record")

def tarball(
        *,
        src: str.type,
        into_dir: str.type,
        user: str.type = "root",
        group: str.type = "root") -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "tarball",
        deps_or_sources = {
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
    src = "artifact",
    into_dir = str.type,
    force_root_ownership = bool.type,
)

def tarball_analyze(
        ctx: "AnalyzeFeatureContext",
        into_dir: str.type,
        user: str.type,
        group: str.type,
        deps_or_sources: {str.type: ["artifact", "dependency"]}) -> FeatureAnalysis.type:
    tarball = deps_or_sources["source"]
    if type(tarball) == "dependency":
        tarball = ensure_single_output(tarball)
    extracted = ctx.actions.declare_output(
        "tarball_" + ctx.unique_action_identifier + "_" + tarball.basename,
        dir = True,
    )
    ctx.actions.run(
        cmd_args(
            ctx.toolchain.antlir2[RunInfo],
            "extract-tarball",
            cmd_args(tarball, format = "--tar={}"),
            cmd_args(extracted.as_output(), format = "--out={}"),
            cmd_args(user, format = "--user={}"),
            cmd_args(group, format = "--group={}"),
        ),
        category = "feature_tarball",
        identifier = "tarball_" + ctx.unique_action_identifier,
        local_only = True,  # needs 'zstd' binary available
    )
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
