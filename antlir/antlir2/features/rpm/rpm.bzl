# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load(
    "//antlir/antlir2/bzl:types.bzl",
    "BuildApplianceInfo",  # @unused Used as type
)
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
    "feature_record",
)
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:structs.bzl", "structs")
load(":plan.bzl", "rpm_planner")

# a fully qualified rpm nevra with epoch will contain a ':' so we have to be a
# little more particular than just checking "if ':' in s"
def _looks_like_label(s: str) -> bool:
    if s.startswith(":"):
        return True
    if ":" in s and "//" in s:
        return True
    return False

__VERSIONLOCK_HARD_ENFORCEMENT_KWARG = select({
    # TODO(vmagro): there are enough broken Controll's that this needs to be
    # temporarily turned off again
    "DEFAULT": False,
    # TODO(vmagro): come up with a better way to handle this, but for now just
    # blocklist the small amount of images that use these non-standard
    # sub-flavo, since there are locked versions that won't exist in these
    # repos.
    "antlir//antlir/antlir2/facebook/flavor/centos9:corp": False,
    "antlir//antlir/antlir2/facebook/flavor/centos9:public-only": False,
    "antlir//antlir/antlir2/os:rhel8": False,
    "antlir//antlir/antlir2/os:rhel8.8": False,
    "antlir//antlir/antlir2/os:rhel9": False,
    "antlir//antlir/antlir2/os:rhel9.2": False,
})

def _install_common(
        action: str,
        *,
        rpms: list[str] = [],
        subjects: list[str | Select] | Select = [],
        deps: list[str | Select] | Select = [],
        subjects_src: str | Select | None = None):
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' (or anything that resolves as a DNF subject -
    see
    https://dnf.readthedocs.io/en/latest/command_ref.html#specifying-packages-label)
    or a buck target that produces a .rpm artifact.

    To ergonomically use `select`, callers must disambiguate between rpm names (or, more accurately, dnf subjects)
    """
    if rpms and (subjects or deps):
        fail("'rpms' cannot be mixed with 'subjects' or 'deps', it causes api ambiguity")

    # make a writable copy if we might need to add to it
    if type(subjects) == "list":
        subjects = list(subjects)
    unnamed_deps_or_srcs = None
    for rpm in rpms:
        if _looks_like_label(rpm):
            if not unnamed_deps_or_srcs:
                unnamed_deps_or_srcs = []
            unnamed_deps_or_srcs.append(rpm)
        else:
            subjects.append(rpm)
    if unnamed_deps_or_srcs and deps:
        fail("impossible, 'unnamed_deps_or_srcs' cannot be populated if 'rpms' is empty")
    if not unnamed_deps_or_srcs:
        unnamed_deps_or_srcs = deps

    return ParseTimeFeature(
        feature_type = "rpm",
        plugin = "antlir//antlir/antlir2/features/rpm:rpm",
        unnamed_deps_or_srcs = unnamed_deps_or_srcs,
        srcs = {
            "subjects_src": subjects_src,
        } if subjects_src else None,
        kwargs = {
            "action": action,
            "subjects": subjects,
            "versionlock_hard_enforce": __VERSIONLOCK_HARD_ENFORCEMENT_KWARG,
        },
        distro_platform_deps = {
            "driver": "antlir//antlir/antlir2/features/rpm:driver",
        },
        exec_deps = {
            "plan": "antlir//antlir/antlir2/features/rpm:plan",
        },
    )

def rpms_install(
        *,
        rpms: list[str] = [],
        subjects: list[str | Select] | Select = [],
        deps: list[str | Select] | Select = [],
        subjects_src: str | Select | None = None):
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like `"systemd"`, a NEVR like
    `"systemd-251.4-1.2.hs+fb.el8"` (or anything that resolves as a [DNF
    subject](https://dnf.readthedocs.io/en/latest/command_ref.html#specifying-packages-label))
    or a buck target that produces a `.rpm` artifact.

    If you want to `select` RPMs, you must explicitly use `subjects` (for DNF
    subjects) or `deps` (for buck targets).
    """
    return _install_common(
        "install",
        rpms = rpms,
        subjects = subjects,
        deps = deps,
        subjects_src = subjects_src,
    )

def rpms_upgrade(
        *,
        rpms: list[str] = [],
        subjects: list[str | Select] | Select = [],
        deps: list[str | Select] | Select = [],
        subjects_src: str | Select | None = None):
    """
    Force an upgrade (if possible, which includes respecting versionlock!) of
    these rpms.

    See [`feature.rpms_install`](#featurerpms_install) for explanations of each
    argument.
    """
    return _install_common(
        "upgrade",
        rpms = rpms,
        subjects = subjects,
        deps = deps,
        subjects_src = subjects_src,
    )

def rpms_remove_if_exists(*, rpms: list[str | Select] | Select):
    """
    Remove RPMs if they are installed

    Elements in `rpms` can be any rpm specifier (name, NEVR, etc). If the rpm is
    not installed, this feature is a no-op.

    :::note
    Dependencies of these rpms may also be removed, but only if no
    explicitly-installed RPM depends on them (in this case, the goal cannot be
    solved and the image build will fail unless you remove those rpms as well).
    :::
    """
    return ParseTimeFeature(
        feature_type = "rpm",
        plugin = "antlir//antlir/antlir2/features/rpm:rpm",
        kwargs = {
            "action": "remove_if_exists",
            "subjects": rpms,
            "versionlock_hard_enforce": __VERSIONLOCK_HARD_ENFORCEMENT_KWARG,
        },
        distro_platform_deps = {
            "driver": "antlir//antlir/antlir2/features/rpm:driver",
        },
        exec_deps = {
            "plan": "antlir//antlir/antlir2/features/rpm:plan",
        },
    )

def rpms_remove(*, rpms: list[str | Select] | Select):
    """
    Remove RPMs if they are installed, fail if they are not installed.

    Elements in `rpms` can be any rpm specifier (name, NEVR, etc). If the rpm is
    not installed, this feature will fail.

    :::note
    Dependencies of these rpms may also be removed, but only if no
    explicitly-installed RPM depends on them (in this case, the goal cannot be
    solved and the image build will fail unless you remove those rpms as well).
    :::
    """
    return ParseTimeFeature(
        feature_type = "rpm",
        plugin = "antlir//antlir/antlir2/features/rpm:rpm",
        kwargs = {
            "action": "remove",
            "subjects": rpms,
            "versionlock_hard_enforce": __VERSIONLOCK_HARD_ENFORCEMENT_KWARG,
        },
        distro_platform_deps = {
            "driver": "antlir//antlir/antlir2/features/rpm:driver",
        },
        exec_deps = {
            "plan": "antlir//antlir/antlir2/features/rpm:plan",
        },
    )

def dnf_module_enable(*, name: str | Select, stream: str | Select):
    """
    Enable this DNF module before resolving the DNF transaction

    See this page for more details about modules
    https://docs.fedoraproject.org/en-US/modularity/using-modules/
    """
    return ParseTimeFeature(
        feature_type = "rpm",
        plugin = "antlir//antlir/antlir2/features/rpm:rpm",
        kwargs = {
            "action": "module_enable",
            "subjects": [selects.apply(
                selects.join(name = name, stream = stream),
                lambda sels: ":".join([sels.name, sels.stream]),
            )],
            "versionlock_hard_enforce": __VERSIONLOCK_HARD_ENFORCEMENT_KWARG,
        },
        distro_platform_deps = {
            "driver": "antlir//antlir/antlir2/features/rpm:driver",
        },
        exec_deps = {
            "plan": "antlir//antlir/antlir2/features/rpm:plan",
        },
    )

action_enum = enum(
    "install",
    "remove",
    "remove_if_exists",
    "upgrade",
    "module_enable",
)

rpm_source_record = record(
    subject = field([str, None], default = None),
    src = field([Artifact, None], default = None),
    subjects_src = field([Artifact, None], default = None),
)

rpm_item_record = record(
    action = action_enum,
    rpm = rpm_source_record,
    feature_label = TargetLabel,
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    rpms = []
    for rpm in ctx.attrs.subjects:
        rpms.append(rpm_source_record(subject = rpm))

    artifacts = []
    for rpm in ctx.attrs.unnamed_deps_or_srcs:
        if isinstance(rpm, Dependency):
            rpm = ensure_single_output(rpm)
        rpms.append(rpm_source_record(src = rpm))
        artifacts.append(rpm)

    if ctx.attrs.subjects_src:
        rpms.append(rpm_source_record(subjects_src = ctx.attrs.subjects_src))
        artifacts.append(ctx.attrs.subjects_src)

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "rpm",
            data = struct(
                items = [
                    rpm_item_record(
                        action = action_enum(ctx.attrs.action),
                        rpm = rpm,
                        feature_label = ctx.label.raw_target(),
                    )
                    for rpm in rpms
                ],
                driver_cmd = ctx.attrs.driver[RunInfo],
                versionlock_hard_enforce = ctx.attrs.versionlock_hard_enforce,
            ),
            required_artifacts = artifacts,
            build_phase = BuildPhase("package_manager"),
            plugin = ctx.attrs.plugin,
            reduce_fn = _reduce_rpm_features,
            planner = rpm_planner(
                plan = ctx.attrs.plan,
                driver_cmd = ctx.attrs.driver[RunInfo],
                versionlock_hard_enforce = ctx.attrs.versionlock_hard_enforce,
            ),
        ),
    ]

rpms_rule = rule(
    impl = _impl,
    attrs = {
        "action": attrs.enum(["install", "remove", "remove_if_exists", "upgrade", "module_enable"]),
        # this is annoying because it's really an exec_dep that we want to run,
        # but it needs to resolve differently depending on the target platform.
        # This is probably a use case for a toolchain_dep, but shoehorning that
        # into antlir2 is extremely tricky, so we can just live with slower
        # aarch64 builds until dnf5 is the only thing we support
        "driver": attrs.dep(providers = [RunInfo]),
        "plan": attrs.exec_dep(providers = [RunInfo]),
        "plugin": attrs.label(),
        "subjects": attrs.list(attrs.string()),
        "subjects_src": attrs.option(attrs.source(), default = None),
        # TODO: refactor this into a more obvious interface
        "unnamed_deps_or_srcs": attrs.list(attrs.one_of(attrs.dep(), attrs.source()), default = []),
        "versionlock_hard_enforce": attrs.bool(default = True),
    },
)

def _reduce_rpm_features(left: feature_record | typing.Any, right: feature_record | typing.Any):
    f = structs.to_dict(left)
    f["analysis"] = structs.to_dict(left.analysis)
    f["analysis"]["data"] = structs.to_dict(f["analysis"]["data"])
    f["analysis"]["data"]["items"] = f["analysis"]["data"]["items"] + right.analysis.data.items
    f["analysis"]["data"] = structs.from_dict(f["analysis"]["data"])
    f["analysis"]["required_artifacts"] = f["analysis"]["required_artifacts"] + right.analysis.required_artifacts
    f["analysis"] = FeatureAnalysis(**f["analysis"])
    return feature_record(**f)
