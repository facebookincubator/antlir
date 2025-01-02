# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbsource//tools/target_determinator/macros:ci.bzl", "ci")
load("@fbsource//tools/target_determinator/macros:fbcode_ci_helpers.bzl", "fbcode_ci")

def disable_dev_mode(labels: list[str]) -> list[str]:
    """ In addition to `use_opt_instead_of_dev`, explicitly replace some modes
    because it won't catch default dev modes. This function is in its own file
    to make oss-disable happy.
    """
    labels += ci.labels(
        ci.replace({
            # Override tags with default dev mode. This is lossy, because we
            # will drop all other modes but opt for simplicity. The goal of VM
            # tests isn't to test all build modes. We just need that one mode
            # works and is fast enough for the VM.
            ci.linux(ci.x86_64()): ci.linux(ci.x86_64(ci.opt())),
            ci.linux(ci.aarch64()): ci.linux(ci.aarch64(ci.opt())),
            # ci.mode() seems to be rather diverse. Try covering everything
            # starting with dev. If there is a matching opt mode, use that. Or
            # we use the vanilla opt.
            ci.linux(ci.mode("fbcode//mode/dev-asan")): ci.linux(ci.mode("fbcode//mode/opt-asan")),
            ci.linux(ci.mode("fbcode//mode/dev-tsan")): ci.linux(ci.mode("fbcode//mode/opt-tsan")),
            ci.linux(ci.mode("fbcode//mode/dev-ubsan")): ci.linux(ci.mode("fbcode//mode/opt-ubsan")),
            # Not sure if the link group issue we had is specific to dev
            # (D51891046). Give opt-lg a chance.
            ci.linux(ci.mode("fbcode//mode/dev-lg")): ci.linux(ci.mode("fbcode//mode/opt-lg")),
            ci.linux(ci.mode("fbcode//mode/dev-cov")): ci.linux(ci.opt()),
            ci.linux(ci.mode("fbcode//mode/dev-nosan")): ci.linux(ci.opt()),
            ci.linux(ci.mode("fbcode//mode/dev-nosan-lg")): ci.linux(ci.opt()),
        }),
        fbcode_ci.use_opt_instead_of_dev(),
    )
    return labels
