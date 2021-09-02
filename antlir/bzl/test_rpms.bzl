# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")

def add_test_rpms(rpmlist):
    return [
        image.rpms_install(rpmlist, flavors = ["antlir_test"]),
    ] + [
        image.rpms_install([], flavors = REPO_CFG.flavor_available),
    ]

def remove_test_rpms(rpmlist):
    return [
        image.rpms_remove_if_exists(rpmlist, flavors = ["antlir_test"]),
    ] + [
        image.rpms_remove_if_exists([], flavors = REPO_CFG.flavor_available),
    ]
