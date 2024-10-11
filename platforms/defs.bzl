def _nativelink_platform(ctx):
    constraints = {}
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)

    configuration = ConfigurationInfo(
        constraints = constraints,
        values = {},
    )

    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = configuration,
        executor_config = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = True,
            use_limited_hybrid = True,
            remote_execution_properties = {
                # "arch": ctx.attrs.cpu_configuration.label.name,
                # "os": ctx.attrs.os_configuration.label.name,
            },
            remote_execution_use_case = "antlir",
            remote_output_paths = "output_paths",
        ),
    )

    return [DefaultInfo(), platform]

nativelink_platform = rule(attrs = {
    "cpu_configuration": attrs.dep(providers = [ConfigurationInfo]),
    "os_configuration": attrs.dep(providers = [ConfigurationInfo]),
}, impl = _nativelink_platform)

def _exec_platforms(ctx):
    return [
        DefaultInfo(),
        ExecutionPlatformRegistrationInfo(
            platforms = [p[ExecutionPlatformInfo] for p in ctx.attrs.platforms],
        ),
    ]

exec_platforms = rule(attrs = {
    "platforms": attrs.list(attrs.dep(providers = [ExecutionPlatformInfo])),
}, impl = _exec_platforms)
