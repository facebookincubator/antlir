# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "MultiFeatureAnalysis",
    "ParseTimeFeature",
    "new_feature_rule",
)
load("//antlir/bzl:stat.bzl", "stat")

def ensure_subdirs_exist(
    *,
    into_dir: str | Select,
    subdirs_to_create: str | Select,
    mode: int | str | Select = 0o755,
    user: str | Select = "root",
    group: str | Select = "root",
    xattrs: dict[str, str] | Select = {},
):
    """
    Ensure directories exist in the image (analogous to `mkdir -p`).

    Args:
        into_dir: Parent directory (must already exist)
        subdirs_to_create: Subdirectories to create under `into_dir`

            These subdirectories may already exist in the image. If so, they
            will be checked to ensure that the `mode` and `user:group` matches
            what is declared here.

        mode: set file mode bits of the newly-created directories
        user: set owning user of the newly-created directories
        group: set owning group of the newly-created directories
        xattrs: extended attributes to set on the leaf directory
    """
    return ParseTimeFeature(
        feature_type = "ensure_dir_exists",
        plugin = "antlir//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists",
        kwargs = {
            "group": group,
            "into_dir": into_dir,
            "mode": mode,
            "subdirs_to_create": subdirs_to_create,
            "user": user,
            "xattrs": xattrs,
        },
    )

def ensure_dirs_exist(*, dirs: str | Select, mode: int | str = 0o755, user: str | Select = "root", group: str | Select = "root"):
    """Equivalent to `ensure_subdirs_exist("/", dirs, ...)`."""
    return ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = dirs,
        mode = mode,
        user = user,
        group = group,
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    mode = stat.mode(ctx.attrs.mode) if ctx.attrs.mode else None
    features = []
    dir = ctx.attrs.into_dir
    components = [c for c in ctx.attrs.subdirs_to_create.split("/") if c]
    for i, component in enumerate(components):
        dir = paths.join(dir, component)
        is_leaf = i == len(components) - 1

        features.append(
            FeatureAnalysis(
                feature_type = "ensure_dir_exists",
                data = struct(
                    dir = dir,
                    mode = mode,
                    user = ctx.attrs.user,
                    group = ctx.attrs.group,
                    xattrs = ctx.attrs.xattrs if is_leaf else {},
                ),
                plugin = ctx.attrs.plugin,
                build_phase = BuildPhase(ctx.attrs.build_phase),
            )
        )
    return [
        DefaultInfo(),
        MultiFeatureAnalysis(
            features = features,
        ),
    ]

ensure_dir_exists_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "build_phase": attrs.enum(BuildPhase.values(), default = "compile"),
        "group": attrs.string(),
        "into_dir": attrs.string(),
        "mode": attrs.one_of(attrs.string(), attrs.int()),
        "subdirs_to_create": attrs.string(),
        "user": attrs.string(),
        "xattrs": attrs.dict(attrs.string(), attrs.string(), default = {}),
    },
)
