# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Expose shell test metadata for an image wrapper without host test discovery."""

def _impl(ctx: AnalysisContext) -> list[Provider]:
    test = ctx.attrs.test
    hidden = list(ctx.attrs.resources)
    if isinstance(test, Dependency):
        if RunInfo in test:
            command = test[RunInfo]
        else:
            command = test[DefaultInfo].default_outputs
            hidden.extend(test[DefaultInfo].other_outputs)
    else:
        command = test
    for dep in ctx.attrs.deps:
        hidden.extend(dep[DefaultInfo].default_outputs)
        hidden.extend(dep[DefaultInfo].other_outputs)

    info = ExternalRunnerTestInfo(
        type = ctx.attrs.type or "custom",
        command = [cmd_args(command, hidden = hidden)] + ctx.attrs.args,
        env = ctx.attrs.env,
        labels = ctx.attrs.labels,
        contacts = ctx.attrs.contacts,
        supports_test_execution_caching = ctx.attrs.supports_test_execution_caching,
    )
    # Buck's OSS runner discovers every root test provider, including targets
    # labelled disabled. Only the image wrapper requests this subtarget.
    return [DefaultInfo(sub_targets = {"test": [DefaultInfo(), info]})]

hidden_sh_test = rule(
    impl = _impl,
    attrs = {
        "args": attrs.list(attrs.arg(), default = []),
        "contacts": attrs.list(attrs.string(), default = []),
        "deps": attrs.list(attrs.dep(), default = []),
        "env": attrs.dict(attrs.string(), attrs.arg(), default = {}),
        "labels": attrs.list(attrs.string(), default = []),
        "resources": attrs.list(attrs.source(), default = []),
        "supports_test_execution_caching": attrs.bool(default = False),
        "test": attrs.one_of(attrs.dep(), attrs.source()),
        "type": attrs.option(attrs.string(), default = None),
    },
)
