# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/package:gpt.bzl?v2_only", antlir2_Partition = "Partition", antlir2_PartitionType = "PartitionType", antlir2_gpt = "gpt")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":gpt.shape.bzl", "gpt_partition_t")

def image_gpt_partition(package, is_esp = False, name = None):
    return gpt_partition_t(
        package = package,
        is_esp = is_esp,
        name = name,
    )

def image_gpt(
        name,
        table,
        disk_guid = None,
        visibility = None,
        build_appliance = None,
        **kwargs):
    visibility = visibility or []
    if antlir2_shim.upgrade_or_shadow_package(
        antlir2 = None,
        fn = antlir2_gpt,
        name = name,
        partitions = [
            antlir2_Partition(
                part.package,
                antlir2_PartitionType("esp") if part.is_esp else antlir2_PartitionType("linux"),
                part.name,
            )
            for part in table
        ],
        disk_guid = disk_guid,
        visibility = visibility,
    ) != "upgrade":
        fail("antlir1 is dead")
