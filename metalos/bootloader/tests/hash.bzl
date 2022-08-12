# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl:hoist.bzl", "hoist")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def pe_hash(name, binary, algorithm = "sha1"):
    '''
    Takes in a PE-COFF binary and outputs a file with the hash
    as computed by the "pesign" binary.
    Algorithm may be anything that pesign supports (eg. sha1, sha256, etc)
    '''
    image.layer(
        name = name + "__pesign_setup",
        parent_layer = flavor_helpers.get_build_appliance(),
        features = [
            feature.rpms_install(["pesign"]),
            feature.install(binary, "/input"),
            feature.ensure_dirs_exist("/out", mode = 0o777),
        ],
        flavor = flavor_helpers.get_antlir_linux_flavor(),
    )

    image.genrule_layer(
        name = name + "__hash",
        parent_layer = ":" + name + "__pesign_setup",
        rule_type = "external_binary",
        antlir_rule = "user-internal",
        cmd = [
            "bash",
            "-uec",
            "pesign -hi /input -d {} | cut -f2 -d' ' > /out/hash".format(algorithm),
        ],
    )

    hoist(
        name = name,
        layer = ":" + name + "__hash",
        path = "/out/hash",
    )
