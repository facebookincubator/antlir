# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":bash.bzl", "wrap_bash_build_in_common_boilerplate")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":oss_shim.bzl", "buck_genrule")
load(":target_helpers.bzl", "antlir_dep")

def extract_rpm_manifest(name, layer, visibility = None, build_appliance = None):
    build_appliance = build_appliance or flavor_helpers.get_build_appliance()

    buck_genrule(
        name = name,
        out = "rpm-manifest.json",
        bash = wrap_bash_build_in_common_boilerplate(
            bash = '''
            $(exe {exe_target}) \
              --output-path "$OUT" \
              --layer $(location {layer}) \
              --build-appliance $(location {build_appliance}) \
            '''.format(
                exe_target = antlir_dep(":rpm-manifest"),
                layer = layer,
                build_appliance = build_appliance,
            ),
            target_name = name,
        ),
        cacheable = False,
        executable = True,
        visibility = visibility,
        antlir_rule = "user-internal",
    )
