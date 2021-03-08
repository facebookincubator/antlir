# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")

def antlir_linux_build_opts():
    return image.opts(
        build_appliance = REPO_CFG.artifact["build_appliance.newest"],
        rpm_installer = "dnf",
    )
