load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")

def host_config(name, **kwargs):
    """
    Macro to build a host config JSON blob.
    This is mainly useful for testing, so for ease of iteration is only
    type-checked at `buck build` time, not at parse time.
    """
    buck_genrule(
        name = name,
        cmd = "echo {} | $(location //metalos/host_configs/tests:serialize-host-struct) > $OUT".format(shell.quote(struct(**kwargs).to_json())),
    )
