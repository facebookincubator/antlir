# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
    "feature_record",
    "new_feature_rule",
)
load("//antlir/bzl:internal_external.bzl", "internal_external")
load("//antlir/bzl:structs.bzl", "structs")
load(":plan.bzl", "apt_planner")

TRIXIE_SUITE = internal_external(
    fb = "fbcode//bot_generated/antlir/snapshot/antlir/antlir2/package_managers/deb/trixie:trixie",
    oss = "antlir//antlir/antlir2/package_managers/deb:trixie",
)

NOBLE_SUITE = internal_external(
    fb = "fbcode//bot_generated/antlir/snapshot/antlir/antlir2/package_managers/deb/noble:noble",
    oss = "antlir//antlir/antlir2/package_managers/deb:noble",
)

def _common(action: str, *, packages: list[str | Select] | Select):
    return ParseTimeFeature(
        feature_type = "apt",
        plugin = "antlir//antlir/antlir2/features/apt:apt",
        kwargs = {
            "action": action,
            "subjects": packages,
        },
        deps = {
            "suite": select({
                "antlir//antlir/antlir2/os:debian-trixie": TRIXIE_SUITE,
                "antlir//antlir/antlir2/os:ubuntu-noble": NOBLE_SUITE,
            }),
        },
        distro_platform_deps = {
            "driver": "antlir//antlir/antlir2/features/apt:driver",
            "resolve": "antlir//antlir/antlir2/features/apt:resolve",
        },
        exec_deps = {
            "plan": "antlir//antlir/antlir2/features/apt:plan",
        },
    )

def apt_install(*, packages: list[str | Select] | Select):
    """
    Install deb packages by name.

    Elements in `packages` are apt package names like `"bash"` or `"systemd"`.
    """
    return _common("install", packages = packages)

def apt_remove_if_exists(*, packages: list[str | Select] | Select):
    """
    Remove deb packages if they are installed.

    Elements in `packages` are apt package names. If a package is not installed,
    this feature is a no-op.
    """
    return _common("remove_if_exists", packages = packages)

def apt_remove(*, packages: list[str | Select] | Select):
    """
    Remove deb packages, fail if they are not installed.

    Elements in `packages` are apt package names. If a package is not installed,
    this feature will fail.
    """
    return _common("remove", packages = packages)

action_enum = enum(
    "install",
    "remove",
    "remove_if_exists",
)

apt_source_record = record(
    subject = str,
)

apt_item_record = record(
    action = action_enum,
    apt = apt_source_record,
    feature_label = TargetLabel,
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    items = [
        apt_item_record(
            action = action_enum(ctx.attrs.action),
            apt = apt_source_record(subject = pkg),
            feature_label = ctx.label.raw_target(),
        )
        for pkg in ctx.attrs.subjects
    ]

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "apt",
            data = struct(
                items = items,
                driver_cmd = ctx.attrs.driver[RunInfo],
            ),
            build_phase = BuildPhase("package_manager"),
            plugin = ctx.attrs.plugin,
            reduce_fn = _reduce_apt_features,
            planner = apt_planner(
                plan = ctx.attrs.plan,
                resolve_cmd = ctx.attrs.resolve[RunInfo],
                suite = ctx.attrs.suite,
            ),
        ),
    ]

apt_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "action": attrs.enum(["install", "remove", "remove_if_exists"]),
        "driver": attrs.dep(providers = [RunInfo]),
        "plan": attrs.exec_dep(providers = [RunInfo]),
        "resolve": attrs.dep(providers = [RunInfo]),
        "subjects": attrs.list(attrs.string()),
        "suite": attrs.dep(),
    },
)

def _reduce_apt_features(left: feature_record | typing.Any, right: feature_record | typing.Any):
    f = structs.to_dict(left)
    f["analysis"] = structs.to_dict(left.analysis)
    f["analysis"]["data"] = structs.to_dict(f["analysis"]["data"])
    f["analysis"]["data"]["items"] = f["analysis"]["data"]["items"] + right.analysis.data.items
    f["analysis"]["data"] = structs.from_dict(f["analysis"]["data"])
    f["analysis"] = FeatureAnalysis(**f["analysis"])
    return feature_record(**f)
