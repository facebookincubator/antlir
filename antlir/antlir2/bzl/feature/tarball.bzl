# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

types.lint_noop()

def tarball(
        *,
        src: str.type,
        into_dir: str.type,
        force_root_ownership: bool.type = False) -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "tarball",
        deps_or_sources = {
            "source": src,
        },
        kwargs = {
            "force_root_ownership": force_root_ownership,
            "into_dir": into_dir,
        },
    )

tarball_record = record(
    source = "artifact",
    into_dir = str.type,
    force_root_ownership = bool.type,
)

def tarball_analyze(
        into_dir: str.type,
        force_root_ownership: bool.type,
        sources: {str.type: "artifact"}) -> FeatureAnalysis.type:
    return FeatureAnalysis(
        data = tarball_record(
            force_root_ownership = force_root_ownership,
            into_dir = into_dir,
            source = sources["source"],
        ),
        required_artifacts = sources["source"],
    )
