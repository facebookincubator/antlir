# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This dict is imported into `//antlir/bzl:constants.bzl` to provide per
# repository configuration.
do_not_use_repo_cfg = {
    "antlir_linux_flavor": "fedora33",
    "artifact_key_to_path": " ".join([
        (k + " " + v)
        for k, v in {
            "build_appliance.newest": "//images/appliance:stable-build-appliance",
            "extractor.common_deps": "//images/appliance:stable-build-appliance",
            "metalos.layer.base": "//images/base:fedora.vm",
            "vm.rootfs.btrfs": "//images/base:fedora.vm.btrfs",
            "vm.rootfs.layer": "//images/base:fedora.vm",
        }.items()
    ]),
    "buck1_tgts_to_flavors": {},
    "flavor_available": " ".join(["fedora33"]),
    "flavor_default": "fedora33",
    # KEEP THIS DICTIONARY SMALL.
    #
    # For each `feature`, we have to emit as many targets as there are
    # elements, because we do not know the version set that the
    # including `image.layer` will use.  This would be fixable if Buck
    # supported providers like Bazel does.
    "flavor_to_config": {
        # Do NOT put this in `flavor_available`.
        "antlir_test": {
            "build_appliance": "//antlir/compiler/test_images:build_appliance_testing",
            "rpm_installer": "dnf",
        },
        "fedora33": {
            "build_appliance": "//images/appliance:stable-build-appliance",
            "rpm_installer": "dnf",
        },
    },
    "host_mounts_allowed_in_targets": " ".join([
        "//images/appliance:host-build-appliance",
        "//images/appliance:host-build-appliance__layer-feature",
    ]),
    "host_mounts_for_repo_artifacts": [],
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
}
