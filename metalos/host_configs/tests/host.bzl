load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:structs.bzl", "structs")

def host_config(name, host_config, **kwargs):
    """
    Macro to build a host config JSON blob.
    This is mainly useful for testing, so for ease of iteration is only
    type-checked at `buck build` time, not at parse time.
    """
    buck_genrule(
        name = name,
        cmd = "echo {} | $(location //metalos/host_configs/tests:serialize-host-struct) > $OUT".format(shell.quote(structs.as_json(struct(**host_config)))),
        **kwargs
    )
    buck_genrule(
        name = name + "-provisioning_config",
        cmd = "echo {} | $(location //metalos/host_configs/tests:serialize-host-struct) provisioning > $OUT".format(shell.quote(structs.as_json(struct(**host_config)))),
        **kwargs
    )
    buck_genrule(
        name = name + "-boot_config",
        cmd = "echo {} | $(location //metalos/host_configs/tests:serialize-host-struct) boot > $OUT".format(shell.quote(structs.as_json(struct(**host_config)))),
        **kwargs
    )
    buck_genrule(
        name = name + "-runtime_config",
        cmd = "echo {} | $(location //metalos/host_configs/tests:serialize-host-struct) runtime > $OUT".format(shell.quote(structs.as_json(struct(**host_config)))),
        **kwargs
    )
