# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This needs to use  to define a UDR.
# @lint-ignore-every BUCKLINT

load("//antlir/buck2/bzl:toolchain.bzl", "AntlirToolchainInfo")

def _impl(ctx: "context") -> ["provider"]:
    toolchain = ctx.attrs.toolchain[AntlirToolchainInfo]
    inner_script = cmd_args(
        cmd_args("#!/bin/bash", "-e", delimiter = " "),
        ctx.attrs.bash,
        "\n",
        delimiter = "\n",
    )
    inner_script, inner_script_hidden_deps = ctx.actions.write(
        "script/inner.sh",
        inner_script,
        allow_args = True,
        is_executable = True,
    )
    inner_script_hidden_deps.append(ctx.attrs.bash)
    for dep in (ctx.attrs.deps_query or []):
        if type(dep) != "dependency":
            fail("unexpected query result '{}'".format(dep))
        inner_script_hidden_deps.extend(dep[DefaultInfo].default_outputs)

    script = cmd_args(
        toolchain.builder,
        "--buck-version=2",
        "--label",
        str(ctx.label.raw_target()),
        "--ensure-artifacts-dir-exists",
        toolchain.artifacts_dir,
        "--volume-for-repo",
        toolchain.volume_for_repo,
        inner_script,
    )

    out = ctx.actions.declare_output("out/" + ctx.attrs.out)
    ctx.actions.run(
        cmd_args([script, "--out", out.as_output()]).hidden(inner_script_hidden_deps),
        category = "antlir_boilerplate_genrule",
        identifier = ctx.attrs.type,
        allow_cache_upload = ctx.attrs.cacheable,
        # All the targets that use this macro are by definition accessing image
        # layers that exist only on the host they were built. This can be
        # removed when we finally have cross-host caching.
        local_only = True,
    )
    return [
        DefaultInfo(
            default_outputs = [out],
            sub_targets = {
                "script/inner.sh": [DefaultInfo(default_outputs = [inner_script])],
            },
        ),
    ]

boilerplate_genrule = rule(
    impl = _impl,
    attrs = {
        "antlir_rule": attrs.option(attrs.string()),
        "bash": attrs.arg(),
        "cacheable": attrs.bool(default = True),
        "deps_query": attrs.option(attrs.query()),
        "labels": attrs.list(attrs.string(), default = []),
        "out": attrs.string(default = "out"),
        "toolchain": attrs.default_only(attrs.toolchain_dep([AntlirToolchainInfo], default = "//antlir/buck2:toolchain")),
        "type": attrs.option(attrs.string()),
    },
)
