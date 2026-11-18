# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//cfg/exec_platform:marker.bzl", "get_exec_platform_marker")
load("@prelude//platforms:defs.bzl", "host_configuration")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    constraints = dict()
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)
    for cv in ctx.attrs.constraints:
        constraints[cv[ConstraintValueInfo].setting.label] = cv[ConstraintValueInfo]
    cfg = ConfigurationInfo(constraints = constraints, values = {})

    name = ctx.label.raw_target()
    platform = ExecutionPlatformInfo(
        label = name,
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = False,
            use_windows_path_separators = ctx.attrs.use_windows_path_separators,
        ),
    )

    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(name), configuration = cfg),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
            exec_marker_constraint = get_exec_platform_marker(),
        ),
    ]

host_platform = rule(
    impl = _impl,
    attrs = {
        "constraints": attrs.list(attrs.dep(providers = [ConstraintValueInfo])),
        "cpu_configuration": attrs.default_only(attrs.dep(providers = [ConfigurationInfo], default = host_configuration.cpu)),
        "os_configuration": attrs.default_only(attrs.dep(providers = [ConfigurationInfo], default = host_configuration.os)),
        "use_windows_path_separators": attrs.default_only(attrs.bool(default = host_info().os.is_windows)),
    },
)
