# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/query.bzl", "query")

def _impl(ctx: "context") -> ["provider"]:
    args = [ctx.attrs.executor[RunInfo]]
    for label in ctx.attrs.includes:
        args += ["--includes", str(label)]
    for label in ctx.attrs.excludes:
        args += ["--excludes", str(label)]
    for target in ctx.attrs.query:
        args += ["--actual", str(target.label)]

    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            command = args,
            type = "custom",
        ),
    ]

query_test = rule(
    impl = _impl,
    attrs = {
        "excludes": attrs.list(attrs.label(), default = []),
        "executor": attrs.default_only(attrs.exec_dep(default = "//antlir/buck2/bzl/tests:query-test")),
        "includes": attrs.list(attrs.label(), default = []),
        "query": attrs.query(),
    },
)

def first_order_deps_test(
        name: str.type,
        label: str.type,
        **kwargs):
    return query_test(
        name = name,
        query = query.diff([query.deps(query.set([label]), depth = 1), query.set([label])]),
        **kwargs
    )
