# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features/oci/oci_cmd:oci_cmd.bzl", "oci_cmd")
load("//antlir/antlir2/features/oci/oci_env:oci_env.bzl", "oci_env")
load("//antlir/antlir2/features/oci/oci_exposed_port:oci_exposed_port.bzl", "oci_exposed_port")
load("//antlir/antlir2/features/oci/oci_label:oci_label.bzl", "oci_label")
load("//antlir/antlir2/features/oci/oci_stop_signal:oci_stop_signal.bzl", "oci_stop_signal")
load("//antlir/antlir2/features/oci/oci_user:oci_user.bzl", "oci_user")
load("//antlir/antlir2/features/oci/oci_volume:oci_volume.bzl", "oci_volume")
load("//antlir/antlir2/features/oci/oci_working_dir:oci_working_dir.bzl", "oci_working_dir")

oci_features = struct(
    cmd = oci_cmd,
    env = oci_env,
    exposed_port = oci_exposed_port,
    label = oci_label,
    stop_signal = oci_stop_signal,
    user = oci_user,
    volume = oci_volume,
    working_dir = oci_working_dir,
)
