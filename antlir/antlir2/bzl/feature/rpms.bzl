# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(
    ":feature_info.bzl",
    "AnalyzeFeatureContext",  # @unused Used as type
    "FeatureAnalysis",
    "ParseTimeFeature",
)

# a fully qualified rpm nevra with epoch will contain a ':' so we have to be a
# little more particular than just checking "if ':' in s"
def _looks_like_label(s: str) -> bool:
    if s.startswith(":"):
        return True
    if ":" in s and "//" in s:
        return True
    return False

def _install_common(
        action: str,
        *,
        rpms: list[str] = [],
        subjects: list[str | Select] | Select = [],
        deps: list[str | Select] | Select = [],
        subjects_src: str | Select | None = None) -> ParseTimeFeature:
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
        plugin = antlir2_dep("features:rpm"),
        unnamed_deps_or_srcs = unnamed_deps_or_srcs,
        srcs = {
            "subjects": subjects_src,
        } if subjects_src else None,
        kwargs = {
            "action": action,
            "subjects": subjects,
        },
        analyze_uses_context = True,
        compatible_with = [
            "//antlir/antlir2/os/package_manager:dnf",
        ],
    )

def rpms_install(*args, **kwargs) -> ParseTimeFeature:
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' (or anything that resolves as a DNF subject -
    see
    https://dnf.readthedocs.io/en/latest/command_ref.html#specifying-packages-label)
    or a buck target that produces a .rpm artifact.

    To ergonomically use `select`, callers must disambiguate between rpm names
    (or, more accurately, dnf subjects)
    """
    return _install_common("install", *args, **kwargs)

def rpms_upgrade(*args, **kwargs) -> ParseTimeFeature:
    """
    Force an upgrade (if possible, which includes respecting versionlock!) of
    these rpms.

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' (or anything that resolves as a DNF subject -
    see
    https://dnf.readthedocs.io/en/latest/command_ref.html#specifying-packages-label)
    or a buck target that produces a .rpm artifact.

    To ergonomically use `select`, callers must disambiguate between rpm names
    (or, more accurately, dnf subjects)
    """
    return _install_common("upgrade", *args, **kwargs)

def rpms_remove_if_exists(*, rpms: list[str | Select] | Select) -> ParseTimeFeature:
    """
    Remove RPMs if they are installed

    Elements in `rpms` can be any rpm specifier (name, NEVR, etc). If the rpm is
    not installed, this feature is a no-op.
    Note that dependencies of these rpms can also be removed, but only if no
    explicitly-installed RPM depends on them (in this case, the goal cannot be
    solved and the image build will fail unless you remove those rpms as well).
    """
    return ParseTimeFeature(
        feature_type = "rpm",
        plugin = antlir2_dep("features:rpm"),
        kwargs = {
            "action": "remove_if_exists",
            "subjects": rpms,
        },
        analyze_uses_context = True,
        compatible_with = [
            "//antlir/antlir2/os/package_manager:dnf",
        ],
    )

action_enum = enum("install", "remove_if_exists", "upgrade")

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

rpms_record = record(
    items = list[rpm_item_record],
)

def rpms_analyze(
        *,
        ctx: AnalyzeFeatureContext,
        plugin: FeaturePluginInfo,
        action: str,
        subjects: list[str],
        srcs: dict[str, Artifact] = {},
        unnamed_deps_or_srcs: list[Dependency | Artifact] = []) -> FeatureAnalysis:
    rpms = []
    for rpm in subjects:
        rpms.append(rpm_source_record(subject = rpm))

    artifacts = []
    for rpm in unnamed_deps_or_srcs:
        if type(rpm) == "dependency":
            rpm = ensure_single_output(rpm)
        rpms.append(rpm_source_record(src = rpm))
        artifacts.append(rpm)

    subjects_src = srcs.get("subjects")
    if subjects_src:
        rpms.append(rpm_source_record(subjects_src = subjects_src))
        artifacts.append(subjects_src)

    return FeatureAnalysis(
        feature_type = "rpm",
        data = rpms_record(
            items = [
                rpm_item_record(
                    action = action_enum(action),
                    rpm = rpm,
                    feature_label = ctx.label.raw_target(),
                )
                for rpm in rpms
            ],
        ),
        required_artifacts = artifacts,
        requires_planning = True,
        build_phase = BuildPhase("package_manager"),
        plugin = plugin,
    )
