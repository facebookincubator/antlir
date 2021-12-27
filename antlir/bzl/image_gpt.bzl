# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":image_utils.bzl", "image_utils")

gpt_partition_t = shape.shape(
    package = shape.target(),
    is_esp = bool,
    name = shape.field(str, optional = True),
)

gpt_t = shape.shape(
    name = str,
    disk_guid = shape.field(
        str,
        optional = True,
    ),
    table = shape.list(gpt_partition_t),
)

def image_gpt_partition(package, is_esp = False, name = None):
    return shape.new(gpt_partition_t, package = package, is_esp = is_esp, name = name)

def image_gpt(
        name,
        table,
        disk_guid = None,
        visibility = None,
        build_appliance = None):
    visibility = visibility or []
    build_appliance = build_appliance or flavor_helpers.default_flavor_build_appliance

    gpt = shape.new(gpt_t, name = name, table = table, disk_guid = disk_guid)
    buck_genrule(
        name = name,
        bash = image_utils.wrap_bash_build_in_common_boilerplate(
            self_dependency = "//antlir/bzl:image_gpt",
            bash = '''
            $(exe //antlir:gpt) \
              --output-path "$OUT" \
              --gpt {opts_quoted} \
              --build-appliance $(query_outputs {build_appliance}) \
            '''.format(
                opts_quoted = shell.quote(shape.do_not_cache_me_json(gpt)),
                build_appliance = build_appliance,
            ),
            rule_type = "image_gpt",
            target_name = name,
        ),
        cacheable = False,
        executable = True,
        visibility = visibility,
        antlir_rule = "user-internal",
    )
