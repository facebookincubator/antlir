# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:types.bzl",
    "BuildApplianceInfo",  # @unused Used as type
    "LayerInfo",
)
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "PlanInfo",
    "Planner",
    "feature_record",  # @unused Used as type
)
load("//antlir/antlir2/package_managers/deb:suite.bzl", "DebSuiteInfo")

def _plan_fn(*, ctx: AnalysisContext, identifier: str, feature: feature_record | typing.Any, resolve_cmd: RunInfo, **kwargs) -> list[PlanInfo]:
    # rootless is always passed by layer.bzl but not needed since we never
    # access a parent layer subvolume
    kwargs.pop("rootless", None)

    items = ctx.actions.declare_output(identifier, "apt/items.json", has_content_based_path = False)
    items = ctx.actions.write_json(items, feature.analysis.data.items, with_inputs = True)

    # Get parent dpkg status from supplements if available
    parent_dpkg_status = None
    if ctx.attrs.parent_layer:
        supplements = ctx.attrs.parent_layer[LayerInfo].supplements
        parent_dpkg_status = supplements.get("apt_dpkg_status")

    res = plan(ctx = ctx, identifier = identifier, items = items, resolve_cmd = resolve_cmd, parent_dpkg_status = parent_dpkg_status, **kwargs)
    return [plan_info(res)]

def plan_info(res: struct) -> PlanInfo:
    dpkg_status_out = res.dpkg_status_out

    def _mutate_supplements(supplements: dict[str, typing.Any]) -> dict[str, typing.Any]:
        supplements["apt_dpkg_status"] = dpkg_status_out
        return supplements

    return PlanInfo(
        id = "apt",
        output = res.plan_json,
        hidden = res.hidden,
        sub_artifacts = {
            "tx": res.tx_file,
        },
        mutate_supplements = _mutate_supplements,
    )

def plan(
    *,
    ctx: AnalysisContext,
    identifier: str,
    items: Artifact | typing.Any,
    label: Label,
    build_appliance: BuildApplianceInfo | typing.Any,
    target_arch: str,
    resolve_cmd: RunInfo,
    plan: Dependency,
    suite: Dependency,
    parent_dpkg_status: Artifact | None = None,
) -> struct:
    tx = ctx.actions.declare_output(identifier, "apt/transaction.json", has_content_based_path = False)
    dpkg_status_out = ctx.actions.declare_output(identifier, "apt/dpkg_status.txt", has_content_based_path = False)

    archive_dir = suite[DebSuiteInfo].archive_dir

    ctx.actions.run(
        cmd_args(
            plan[RunInfo],
            cmd_args(label, format = "--label={}"),
            cmd_args(build_appliance.dir, format = "--build-appliance={}"),
            cmd_args(archive_dir, format = "--archive-dir={}"),
            cmd_args(target_arch, format = "--target-arch={}"),
            cmd_args(items, format = "--items={}"),
            cmd_args(resolve_cmd, format = "--resolve-cmd={}"),
            cmd_args(suite[DebSuiteInfo].base_variant, format = "--base-variant={}"),
            cmd_args(tx.as_output(), format = "--out={}"),
            cmd_args(dpkg_status_out.as_output(), format = "--dpkg-status-out={}"),
            cmd_args(parent_dpkg_status, format = "--dpkg-status={}") if parent_dpkg_status else cmd_args(),
        ),
        category = "apt_plan",
        identifier = identifier,
    )

    debs_dir = suite[DebSuiteInfo].debs_dir
    if not debs_dir:
        debs_dir = ctx.actions.declare_output(identifier, "apt/debs", dir = True, has_content_based_path = False)

        # Dynamically materialize the resolved .deb files after the plan
        # action produces the transaction JSON. For snapshotted suites the
        # .debs come straight from the suite's member artifacts (downloaded
        # once on the repo target and shared by every image) instead of
        # separate per-image downloads.
        ctx.actions.dynamic_output_new(
            _download_debs(
                suite = suite[DebSuiteInfo],
                tx = tx,
                debs_dir = debs_dir.as_output(),
            ),
        )

    plan_json = ctx.actions.declare_output(identifier, "apt/plan.json", has_content_based_path = False)
    out = ctx.actions.write_json(
        plan_json,
        struct(
            tx_file = tx,
            build_appliance = build_appliance.dir,
            archive_dir = archive_dir,
            debs_dir = debs_dir,
        ),
        with_inputs = True,
    )

    return struct(
        plan_json = plan_json,
        hidden = [out],
        tx_file = tx,
        dpkg_status_out = dpkg_status_out,
    )

def _download_debs_impl(actions: AnalysisActions, suite: DebSuiteInfo, tx: ArtifactValue, debs_dir: OutputArtifact) -> list[Provider]:
    transaction = tx.read_json()
    debs_map = {}
    for pkg_info in transaction.get("install", []):
        pkg = pkg_info["package"]
        if not pkg["filename"]:
            continue
        debs_map[pkg["filename"]] = suite.get_deb(actions, pkg)
    actions.symlinked_dir(debs_dir, debs_map)
    return []

_download_debs = dynamic_actions(
    impl = _download_debs_impl,
    attrs = {
        "debs_dir": dynattrs.output(),
        "suite": dynattrs.value(DebSuiteInfo),
        "tx": dynattrs.artifact_value(),
    },
)

def apt_planner(*, plan: Dependency, resolve_cmd: RunInfo, suite: Dependency) -> Planner:
    return Planner(
        fn = _plan_fn,
        parent_layer_contents = False,
        build_appliance = True,
        dnf = False,
        label = True,
        flavor = False,
        target_arch = True,
        kwargs = {
            "plan": plan,
            "resolve_cmd": resolve_cmd,
            "suite": suite,
        },
    )
