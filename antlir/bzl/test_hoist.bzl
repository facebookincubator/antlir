# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":constants.bzl", "REPO_CFG")
load(":hoist.bzl", "hoist")
load(":image.bzl", "image")
load(":oss_shim.bzl", "python_unittest")

def test_hoist(name):
    image.layer(
        name = "{}-base-layer".format(name),
        flavor = REPO_CFG.antlir_linux_flavor,
        features = [image.rpms_install([
            "coreutils",
            "findutils",
        ])],
    )

    image.genrule_layer(
        name = name + "-test-layer",
        parent_layer = ":{}-base-layer".format(name),
        rule_type = "build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "bash",
            "-uec",
            """
            set -eo pipefail

            mkdir test
            pushd test

            touch file1
            touch file2.rpm

            mkdir folder1
            touch folder1/file1.rpm
            touch folder1/file2
            """,
        ],
    )

    hoist(
        name = name + "-simple-file",
        layer = "{}-test-layer".format(name),
        path = "test/file1",
    )

    hoist(
        name = name + "-simple-out-file",
        layer = "{}-test-layer".format(name),
        path = "test/file1",
        force_dir = True,
    )

    hoist(
        name = name + "-simple-folder",
        layer = "{}-test-layer".format(name),
        path = "test/folder1",
    )

    hoist(
        name = name + "-simple-selector",
        layer = "{}-test-layer".format(name),
        path = "test",
        selector = [
            "-mindepth 1 -maxdepth 1",
        ],
        force_dir = True,
    )

    hoist(
        name = name + "-selector-flat",
        layer = "{}-test-layer".format(name),
        path = "test",
        selector = [
            "-name '*.rpm'",
        ],
        force_dir = True,
    )

    python_unittest(
        name = name,
        srcs = ["test_hoist.py"],
        resources = {
            ":{}-simple-file".format(name): "test_simple_file",
            ":{}-simple-out-file".format(name): "test_out_file",
            ":{}-simple-folder".format(name): "test_simple_folder",
            ":{}-simple-selector".format(name): "test_simple_selector",
            ":{}-selector-flat".format(name): "test_selector_flat",
        },
    )
