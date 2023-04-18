# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

action_enum = enum("install", "remove_if_exists")

# a fully qualified rpm nevra with epoch will contain a ':' so we have to be a
# little more particular than just checking "if ':' in s"
def _looks_like_label(s: str.type) -> bool.type:
    if s.startswith(":"):
        return True
    if ":" in s and "//" in s:
        return True
    return False

def rpms_install(*, rpms: [str.type]):
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' or a buck target that produces a .rpm artifact.
    """
    return InlineFeatureInfo(
        feature_type = "rpm",
        sources = {"rpm_" + str(i): r for i, r in enumerate(rpms) if _looks_like_label(r)},
        kwargs = {
            "action": action_enum("install"),
            "rpm_names": [r for r in rpms if not _looks_like_label(r)],
        },
    )

def rpms_remove_if_exists(*, rpms: [str.type]):
    """
    Remove RPMs if they are installed

    Elements in `rpms` can be any rpm specifier (name, NEVR, etc). If the rpm is
    not installed, this feature is a no-op.
    Note that dependencies of these rpms can also be removed, but only if no
    explicitly-installed RPM depends on them (in this case, the goal cannot be
    solved and the image build will fail unless you remove those rpms as well).
    """
    return InlineFeatureInfo(
        feature_type = "rpm",
        kwargs = {
            "action": action_enum("remove_if_exists"),
            "rpm_names": rpms,
        },
    )

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

def rpms_to_json(
        action: str.type,
        rpm_names: [str.type],
        sources: {str.type: "artifact"} = {}) -> rpms_record.type:
    rpms = []
    for rpm in rpm_names:
        rpms.append(rpm_source_record(name = rpm, source = None))
    for rpm in sources.values():
        rpms.append(rpm_source_record(source = rpm, name = None))

    return rpms_record(
        items = [
            rpm_item_record(
                action = action_enum(action),
                rpm = rpm,
            )
            for rpm in rpms
        ],
    )
