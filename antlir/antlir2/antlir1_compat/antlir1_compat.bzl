# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Compatibility shims for antlir1->antlir2 migration
This file must load on both buck1 and buck2
"""

load("@fbcode_macros//build_defs:native_rules.bzl", "alias")
load("//antlir/antlir2/bzl:lazy.bzl", "lazy")
load("//antlir/bzl:build_defs.bzl", "get_visibility", "is_buck2")
load("//antlir/bzl:constants.bzl", "BZL_CONST")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:image_layer_utils.bzl", "image_layer_utils")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

# THAR BE DRAGONS
# DO NOT ADD ANYTHING HERE WITHOUT THE APPROVAL OF @vmagro OR @lsalis
# THIS IS FULL OF FOOTGUNS AND YOU SHOULDN'T USE IT WITHOUT KNOWING EXACTLY WHAT
# YOU'RE DOING
_ALLOWED_LABELS = (
    "fbcode//antlir/antlir2/antlir1_compat/tests:antlir1-layer",
    "fbcode//metalos/os/vm:rootfs.antlir1",
    "fbcode//os_foundation/metalos/impl/centos8:basesystem.rc.antlir1",
    "fbcode//os_foundation/metalos/impl/centos9:basesystem.rc.antlir1",
)

_ALLOWED_PACKAGES = (
    "fbcode//tupperware/image/base/impl:",
)

def _make_cmd(location, force_flavor):
    return """
        set -ex
        location={location}
        dst_rel="$subvolume_wrapper_dir/volume"
        dst_abs="$SUBVOLUMES_DIR/$dst_rel"
        sudo btrfs subvolume snapshot "$location" "$dst_abs"
        sudo mkdir -p "$dst_abs/.meta"
        echo -n "{force_flavor}" | sudo tee "$dst_abs/.meta/flavor"
        uuid=`sudo btrfs subvolume show "$dst_abs" | grep UUID: | grep -v "Parent UUID:" | grep -v "Received UUID:" | cut -f5`
        jq --null-input \\
            --arg subvolume_rel_path "$dst_rel" \\
            --arg uuid "$uuid" \\
            --arg hostname "$HOSTNAME" \\
            '{{"subvolume_rel_path": $subvolume_rel_path, "btrfs_uuid": $uuid, "hostname": $hostname, "build_appliance_path": "/"}}' \\
            > "$layer_json"
    """.format(
        location = location,
        force_flavor = force_flavor,
    )

def _common(
        name,
        location,
        rule_type,
        force_flavor,
        layer,
        antlir_rule = "user-facing",
        **kwargs):
    target = normalize_target(":" + name)
    if target not in _ALLOWED_LABELS and not lazy.any(lambda pkg: target.startswith(pkg), _ALLOWED_PACKAGES):
        fail("'{}' has not been approved for use with antlir2's compat mode".format(target))
    features_for_layer = name + "--antlir2-inner" + BZL_CONST.layer_feature_suffix
    feature.new(
        name = features_for_layer,
        features = [],
    )
    image_layer_utils.image_layer_impl(
        _layer_name = name + "--antlir2-inner",
        _rule_type = rule_type,
        _make_subvol_cmd = _make_cmd(
            location = location,
            force_flavor = force_flavor,
        ),
        # sorry buck1 users, builds might be stale, deal with it or move to
        # buck2 and enjoy faster, more correct builds ;p
        _deps_query = None,
        antlir_rule = "user-internal",
        visibility = [normalize_target(":" + name)],
    )
    image.layer(
        name = name,
        antlir_rule = antlir_rule,
        parent_layer = ":" + name + "--antlir2-inner",
        flavor = force_flavor,
        antlir2 = False,
        **kwargs
    )
    alias(
        name = name + ".antlir2",
        actual = layer,
        visibility = get_visibility(kwargs.get("visibility", None)),
    )

def _export_for_antlir1_buck1(name, layer, **kwargs):
    _common(
        name,
        location = """`buck2 build --show-full-json-output "{full_label}" | jq -r '.["{full_label}"]'`""".format(
            full_label = normalize_target(layer),
        ),
        rule_type = "antlir2_buck1_compat",
        layer = layer,
        **kwargs
    )

def _export_for_antlir1_buck2(name, layer, **kwargs):
    _common(
        name,
        location = "$(location {})".format(layer),
        rule_type = "antlir2_compat",
        layer = layer,
        **kwargs
    )

export_for_antlir1 = _export_for_antlir1_buck2 if is_buck2() else _export_for_antlir1_buck1
