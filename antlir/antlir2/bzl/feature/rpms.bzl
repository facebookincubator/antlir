# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

types.lint_noop()

# a fully qualified rpm nevra with epoch will contain a ':' so we have to be a
# little more particular than just checking "if ':' in s"
def _looks_like_label(s: str.type) -> bool.type:
    if s.startswith(":"):
        return True
    if ":" in s and "//" in s:
        return True
    return False

def rpms_install(*, rpms: types.or_selector([types.or_selector(str.type)])) -> ParseTimeFeature.type:
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' or a buck target that produces a .rpm artifact.
    """

    return ParseTimeFeature(
        feature_type = "rpm",
        # TODO: this means that rpm installation from buck deps will not work
        # with selects inside of a list of `rpms`, however, one can put the
        # entire list behind a `select` and use a separate rpms_install feature,
        # so this is not preventing anything, it's just a little less convenient
        deps_or_sources = {"rpm_" + str(i): r for i, r in enumerate(rpms) if (types.is_string(r) and _looks_like_label(r))},
        kwargs = {
            "action": "install",
            "rpm_names": [r for r in rpms if not _looks_like_label(r)],
        },
    )

def rpms_remove_if_exists(*, rpms: [str.type]) -> ParseTimeFeature.type:
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
            "rpm_names": rpms,
        },
    )

action_enum = enum("install", "remove_if_exists")

rpm_source_record = record(
    name = [str.type, None],
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
        rpm_names: [str.type],
        deps_or_sources: {str.type: ["dependency", "artifact"]} = {}) -> FeatureAnalysis.type:
    rpms = []
    for rpm in rpm_names:
        rpms.append(rpm_source_record(name = rpm, source = None))

    artifacts = []
    for rpm in deps_or_sources.values():
        if type(rpm) == "dependency":
            rpm = ensure_single_output(rpm)
        rpms.append(rpm_source_record(source = rpm, name = None))
        artifacts.append(rpm)

    return FeatureAnalysis(
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
    )
