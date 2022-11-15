# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":bash.bzl", "boilerplate_genrule")
load(":build_defs.bzl", "get_visibility")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":target_helpers.bzl", "antlir_dep")

def extract_rpm_manifest(name, layer, visibility = None, build_appliance = None):
    build_appliance = build_appliance or flavor_helpers.get_build_appliance()

    boilerplate_genrule(
        name = name,
        out = "rpm-manifest.json",
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
        cacheable = False,
        visibility = get_visibility(visibility),
        antlir_rule = "user-internal",
    )
