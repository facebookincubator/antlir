# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode_macros//build_defs:native_rules.bzl", "buck_genrule", "buck_sh_test")
load("@fbsource//tools/build_defs/buck2:is_buck2.bzl", "is_buck2")
load("//antlir/facebook/eden_access:with_fbcode.bzl", "cmd_with_fbcode_genrule")

def test_flavor_aliasing_disabled(targets):
    #
    # For the specified target verify that no flavor aliasing will
    # occur. We do this to ensure that those targets are not affected
    # by flavoring aliasing (when it's enabled). We can check this by
    # enabling the `antlir.fail-on-flavor-aliasing` compile time option
    # which will cause any calls to do flavor alias lookups to fail.
    #

    noop_cmds = """
cat > $OUT <<EOF
#!/bin/sh
true
EOF
chmod a+x $OUT
"""

    test_cmds = cmd_with_fbcode_genrule(
        cmd = [
            "buck2",
            "targets",
            "-c antlir.fail-on-flavor-aliasing=1",
            targets,
        ],
        literal_preamble = """
# Set NO_BUCKD because we're using an isolated transient checkout.
NO_BUCKD=1; export NO_BUCKD
""",
        read_only = False,  # Buck writes to `.buckd` e.g.
    )

    buck_genrule(
        name = "test-fail-on-flavor-aliasing.sh",
        out = "test-fail-on-flavor-aliasing.sh",
        #
        # We can only do this test with buck2 since it only expands
        # direct target dependencies (where as buck1 expands all target
        # dependencies in all loaded files). So if we're running with
        # buck1 this test becomes a no-op.
        #
        bash = test_cmds if is_buck2() else noop_cmds,
        cacheable = False,
        executable = True,
        labels = ["uses_sudo", "antlir_macros"],
    )

    # Test that all targets explicitly disable flavor aliasing.
    buck_sh_test(
        name = "test-fail-on-flavor-aliasing",
        test = ":test-fail-on-flavor-aliasing.sh",
    )
