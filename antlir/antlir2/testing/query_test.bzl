# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    query_labels = {d.label.raw_target(): True for d in ctx.attrs.query}
    missing_contains = []
    found_excludes = []
    passed = True
    for label in ctx.attrs.contains:
        label = label.raw_target()
        if label not in query_labels:
            passed = False
            missing_contains.append(label)
    for label in ctx.attrs.excludes:
        label = label.raw_target()
        if label in query_labels:
            passed = False
            found_excludes.append(label)

    script = ctx.actions.write(
        "script.sh",
        cmd_args(
            "#!/bin/bash",
            "set -e",
            cmd_args(
                "echo 'targets that should not be present:'",
                cmd_args(found_excludes, format = "echo '  {}'"),
            ) if found_excludes else cmd_args(),
            cmd_args(
                "echo 'targets that are missing:'",
                cmd_args(missing_contains, format = "echo '  {}'"),
            ) if missing_contains else cmd_args(),
            "echo 'full query results:'",
            cmd_args(query_labels.keys(), format = "echo '  {}'"),
            "exit 1" if not passed else cmd_args(),
            delimiter = "\n",
        ),
        is_executable = True,
    )
    return [
        DefaultInfo(),
        RunInfo(cmd_args(script)),
        ExternalRunnerTestInfo(
            command = [script],
            type = "custom",
        ),
    ]

_query_test = rule(
    impl = _impl,
    attrs = {
        "contains": attrs.list(
            attrs.label(),
            default = [],
            doc = "Assert that these labels are included in the query results",
        ),
        "excludes": attrs.list(
            attrs.label(),
            default = [],
            doc = "Assert that these labels are not included in the query results",
        ),
        "query": attrs.query(),
    },
)

query_test = rule_with_default_target_platform(_query_test)
