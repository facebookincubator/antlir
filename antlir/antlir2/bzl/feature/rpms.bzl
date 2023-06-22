# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

# a fully qualified rpm nevra with epoch will contain a ':' so we have to be a
# little more particular than just checking "if ':' in s"
def _looks_like_label(s: str.type) -> bool.type:
    if s.startswith(":"):
        return True
    if ":" in s and "//" in s:
        return True
    return False

def rpms_install(
        *,
        rpms: [str.type] = [],
        subjects: [[[str.type, "selector"]], "selector"] = [],
        deps: [[[str.type, "selector"]], "selector"] = []) -> ParseTimeFeature.type:
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
        unnamed_deps_or_srcs = unnamed_deps_or_srcs,
        kwargs = {
            "action": "install",
            "subjects": subjects,
        },
    )

def rpms_remove_if_exists(*, rpms: [[[str.type, "selector"]], "selector"]) -> ParseTimeFeature.type:
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
        kwargs = {
            "action": "remove_if_exists",
            "subjects": rpms,
        },
    )

action_enum = enum("install", "remove_if_exists")

rpm_source_record = record(
    subject = [str.type, None],
    source = ["artifact", None],
)

rpm_item_record = record(
    action = action_enum.type,
    rpm = rpm_source_record.type,
)

rpms_record = record(
    items = [rpm_item_record.type],
)

def rpms_analyze(
        action: str.type,
        subjects: [str.type],
        unnamed_deps_or_srcs: [["dependency", "artifact"]] = []) -> FeatureAnalysis.type:
    rpms = []
    for rpm in subjects:
        rpms.append(rpm_source_record(subject = rpm, source = None))

    artifacts = []
    for rpm in unnamed_deps_or_srcs:
        if type(rpm) == "dependency":
            rpm = ensure_single_output(rpm)
        rpms.append(rpm_source_record(source = rpm, subject = None))
        artifacts.append(rpm)

    return FeatureAnalysis(
        feature_type = "rpm",
        data = rpms_record(
            items = [
                rpm_item_record(
                    action = action_enum(action),
                    rpm = rpm,
                )
                for rpm in rpms
            ],
        ),
        required_artifacts = artifacts,
        requires_planning = True,
        build_phase = BuildPhase("package_manager"),
    )
