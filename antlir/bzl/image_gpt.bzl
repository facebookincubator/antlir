# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/antlir2/bzl/package:gpt.bzl?v2_only", antlir2_Partition = "Partition", antlir2_PartitionType = "PartitionType", antlir2_gpt = "gpt")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":bash.bzl", "wrap_bash_build_in_common_boilerplate")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":gpt.shape.bzl", "gpt_partition_t", "gpt_t")

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
        fake_buck1 = struct(
            fn = antlir2_shim.fake_buck1_target,
            name = name,
        ),
    ) == "upgrade":
        return

    build_appliance = build_appliance or flavor_helpers.get_build_appliance()

    gpt = gpt_t(name = name, table = table, disk_guid = disk_guid)
    buck_genrule(
        name = name,
        bash = wrap_bash_build_in_common_boilerplate(
            bash = '''
            $(exe //antlir:gpt) \
              --output-path "$OUT" \
              --gpt {opts_quoted} \
              --build-appliance $(query_outputs {build_appliance}) \
            '''.format(
                opts_quoted = shell.quote(shape.do_not_cache_me_json(gpt)),
                build_appliance = build_appliance,
            ),
            target_name = name,
        ),
        cacheable = False,
        visibility = visibility,
        antlir_rule = "user-internal",
        **kwargs
    )
