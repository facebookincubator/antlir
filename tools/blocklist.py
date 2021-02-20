# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# flake8: noqa

import re

blocklist = [
    "//antlir:test-subvol-utils - test_mark_readonly_and_send_to_new_loopback",
    "//antlir:test-subvol-utils - test_mark_readonly_and_send_to_new_loopback_with_multi_pass",
    # Not sure why these are failing in OSS, there seems to be something broken
    # with the shadow code on the OSS BA
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_update_shadowed_file_booted",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_update_shadowed_file_non_booted",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_install_via_default_shadowed_installer",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_install_via_manually_shadowed_installer",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_install_via_nondefault_snapshot",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_install_via_nondefault_snapshot_no_shadowing",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_update_shadowed_file_booted",
    "//antlir/nspawn_in_subvol/plugins:test-rpm-installer-shadow-paths - test_update_shadowed_file_non_booted",
    "//antlir/rpm:test-yum-dnf-from-snapshot-shadowed - test_update_shadowed",
    "//antlir/rpm:test-yum-dnf-from-snapshot-unshadowed - test_update_shadowed",
    # These tests pass locally on my Arch Desktop, but fail on GH Actions
    "//antlir/compiler/items:test-items - test_receive_sendstream",
    "//antlir/compiler/items:test-rpm-action - test_rpm_action_item_auto_downgrade",
    "//antlir/compiler:test-image-layer - test_foreign_layer",
    "//antlir/compiler:test-image-layer - test_layer_from_demo_sendstreams",
    "//antlir/rpm:test-rpm-metadata - test_rpm_metadata_from_subvol",
    "//antlir:test-subvol-utils - test_receive",
    "//antlir:test-unshare - test_pid_namespace",
    # This is heavily dependent on build settings, and we only care about the
    # internal build size (for now anyway)
    "//antlir/linux/bootloader:base-size - sh_test",
]

blocklist = [re.compile(b) for b in blocklist]
