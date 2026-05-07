# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features/oci/oci_env:oci_env.bzl", "oci_env")
load("//antlir/antlir2/features/oci/oci_label:oci_label.bzl", "oci_label")
load("//antlir/antlir2/features/oci/oci_user:oci_user.bzl", "oci_user")

oci_features = struct(
    env = oci_env,
    label = oci_label,
    user = oci_user,
)
