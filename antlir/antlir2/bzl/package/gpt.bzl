# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

GptPartitionSource = provider(fields = ["src"])

PartitionType = enum("linux", "esp")

def Partition(
        src: str,
        type: PartitionType.type = PartitionType("linux"),
        label: str | None = None):
    return (src, type.value, label, "_internal_came_from_package_fn")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    partitions = []
    for src, type, label, _token in ctx.attrs.partitions:
        partitions.append({
            "label": label,
            "src": src[GptPartitionSource].src,
            "type": type,
        })

    if not partitions:
        fail("must have at least one partition")

    if ctx.attrs.block_size not in (512, 4096):
        fail("invalid block size: {}".format(ctx.attrs.block_size))

    spec = ctx.actions.write_json(
        "spec.json",
        {
            "gpt": {
                "block_size": str(ctx.attrs.block_size),
                "disk_guid": ctx.attrs.disk_guid,
                "partitions": partitions,
            },
        },
        with_inputs = True,
    )
    out = ctx.actions.declare_output("package.gpt")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(out.as_output(), format = "--out={}"),
        ),
        category = "antlir2_gpt",
        local_only = True,  # local subvol for ba
    )
    return [DefaultInfo(out)]

_gpt = rule(
    impl = _impl,
    attrs = {
        "block_size": attrs.int(default = 4096, doc = "block size of the gpt layout in bytes"),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "disk_guid": attrs.option(attrs.string(), default = None),
        "partitions": attrs.list(
            attrs.tuple(
                attrs.dep(providers = [GptPartitionSource]),
                attrs.enum(PartitionType.values(), default = "linux"),
                attrs.option(attrs.string()),
                attrs.enum(["_internal_came_from_package_fn"]),
            ),
        ),
        "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_packager:antlir2-packager")),
    },
)

gpt = rule_with_default_target_platform(_gpt)
