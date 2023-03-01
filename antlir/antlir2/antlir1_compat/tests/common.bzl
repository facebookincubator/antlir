# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# targets common to the buck1 and buck2 setup

load("//antlir/antlir2/antlir1_compat:antlir1_compat.bzl", "export_for_antlir1")
load("//antlir/bzl:image.bzl", "image")

def expand_common_targets():
    export_for_antlir1(
        name = "antlir1-layer",
        layer = ":antlir2-layer",  # this is from TARGETS.v2
        runtime = [
            "container",
            "systemd",
        ],
        force_flavor = "centos9",
    )

    image.layer(
        name = "child",
        parent_layer = ":antlir1-layer",
        flavor = "centos9",
        features = [
            # @oss-disable
        ],
    )
    image.python_unittest(
        name = "child-test",
        layer = ":child",
        srcs = ["test.py"],
        run_as_user = "root",
    )
    image.python_unittest(
        name = "child-test-boot",
        layer = ":child",
        srcs = ["test.py"],
        run_as_user = "root",
        env = {"BOOT": "1"},
        boot = True,
    )
