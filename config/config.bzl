# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This dict is imported into `//antlir/bzl:constants.bzl` to provide per
# repository configuration.
do_not_use_repo_cfg = {
    "artifact_key_to_path": " ".join([
        (k + " " + v)
        for k, v in {
            "build_appliance.newest": "//images/appliance:stable-build-appliance",
            "extractor.common_deps": "//images/appliance:stable-build-appliance",
            "vm.rootfs.btrfs.rc": "//images/base:fedora.vm.btrfs",
            "vm.rootfs.btrfs.stable": "//images/base:fedora.vm.btrfs",
            "vm.rootfs.layer.rc": "//images/base:fedora.vm",
            "vm.rootfs.layer.stable": "//images/base:fedora.vm",
        }.items()
    ]),
    "build_appliance_default": "//images/appliance:stable-build-appliance",
    # Future: Once we can guarantee `libcap-ng` to be at least 0.8, add
    # this in.
    #
    # Also check this issue to see if this can be detected from
    # `cap-ng.h` instead -- once both OSS and FB builds can be
    # guaranteed to have this issue fixed, we can move the conditonal
    # compilation into the `.c` file and remove this config.
    #   https://github.com/stevegrubb/libcap-ng/issues/20
    #
    # "libcap_ng_compiler_flags": "-DCAPNG_SUPPORTS_AMBIENT=1",
    "flavor_default": "fedora33",
    "flavor_to_config": {
        "fedora33": {
            "build_appliance": "//images/appliance:stable-build-appliance",
            "rpm_installer": "dnf",
        },
    },
    "host_mounts_allowed_in_targets": " ".join([
        "//images/appliance:host-build-appliance",
        "//images/appliance:features-for-layer-host-build-appliance",
    ]),
    "host_mounts_for_repo_artifacts": [],
    "rpm_installer_default": "dnf",
    "rpm_installers_supported": " ".join([
        "dnf",
    ]),
}
