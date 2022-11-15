# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:shape.bzl", "shape")
load(":bash.bzl", "boilerplate_genrule")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":gpt.shape.bzl", "gpt_partition_t", "gpt_t")

def image_gpt_partition(package, is_esp = False, is_bios_boot = False, name = None):
    return gpt_partition_t(
        package = package,
        is_esp = is_esp,
        is_bios_boot = is_bios_boot,
        name = name,
    )

def image_gpt(
        name,
        table,
        disk_guid = None,
        visibility = None,
        build_appliance = None):
    visibility = visibility or []
    build_appliance = build_appliance or flavor_helpers.get_build_appliance()

    gpt = gpt_t(name = name, table = table, disk_guid = disk_guid)
    boilerplate_genrule(
        name = name,
        bash = '''
            $(exe //antlir:gpt) \
              --output-path "$OUT" \
              --gpt {opts_quoted} \
              --build-appliance $(query_outputs {build_appliance}) \
            '''.format(
            opts_quoted = shell.quote(shape.do_not_cache_me_json(gpt)),
            build_appliance = build_appliance,
        ),
        cacheable = False,
        visibility = visibility,
        antlir_rule = "user-internal",
    )
