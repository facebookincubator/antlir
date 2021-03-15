# Copyright (c) Facebook, Inc. and its affiliates.
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
            "vm.rootfs.btrfs.rc": "//images/base:fedora.vm.btrfs",
            "vm.rootfs.btrfs.stable": "//images/base:fedora.vm.btrfs",
            "vm.rootfs.layer.rc": "//images/base:fedora.vm",
            "vm.rootfs.layer.stable": "//images/base:fedora.vm",
        }.items()
    ]),
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

# This defines the `platform` that is used when building
# artifacts that should target the host that the build is
# running on.  In most cases this matters in places where
# an `$(exe ...)` resolves to a type that needs to know
# to know what target plarform should be.  Since
# Antlir only works on linux-x86_64 hosts that is the
# default.
DEFAULT_HOST_PLATFORM = "config//platform:linux-x86_64"

# Define the mapping between a build appliance and the platform
# that should be used.
_PLATFORM_TO_BUILD_APPLIANCE_MAP = {
    "fedora33-x86_64": "//images/appliance:stable-build-appliance",
}
_BUILD_APPLIANCE_TO_PLATFORM_MAP = {value: key for key, value in _PLATFORM_TO_BUILD_APPLIANCE_MAP.items()}


def get_platform_for_build_appliance(build_appliance):
    return "config//platform:{}".format(
        _BUILD_APPLIANCE_TO_PLATFORM_MAP.get(build_appliance, "linux-x86_64")
    )

def get_build_appliance_for_platform(platform):
    return _PLATFORM_TO_BUILD_APPLIANCE_MAP.get(platform, None)

def get_all_platforms():
    return _PLATFORM_TO_BUILD_APPLIANCE_MAP.keys()
