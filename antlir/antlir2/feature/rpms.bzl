# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "InlineFeatureInfo")

def rpms_install(*, rpms: [str.type]):
    """
    Install RPMs by identifier or .rpm src

    Elements in `rpms` can be an rpm name like 'systemd', a NEVR like
    'systemd-251.4-1.2.hs+fb.el8' or a buck target that produces a .rpm artifact.
    """
    return InlineFeatureInfo(
        feature_type = "rpm",
        sources = {"rpm_" + str(i): r for i, r in enumerate(rpms) if ":" in r},
        kwargs = {
            "action": "install",
            "rpm_names": [r for r in rpms if ":" not in r],
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
            "action": "remove_if_exists",
            "rpm_names": rpms,
        },
    )

_action_enum = enum("install", "remove_if_exists")
types.lint_noop(_action_enum)

def rpms_to_json(
        action: _action_enum.type,
        rpm_names: [str.type],
        sources: {str.type: "artifact"} = {}) -> {str.type: ""}:
    rpms = []
    for rpm in rpm_names:
        rpms.append({"name": rpm})
    for rpm in sources.values():
        rpms.append({"source": rpm})

    return {
        "items": [
            {
                "action": action,
                "rpm": rpm,
            }
            for rpm in rpms
        ],
    }
