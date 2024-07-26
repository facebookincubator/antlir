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
        type: PartitionType = PartitionType("linux"),
        label: str | None = None,
        alignment: int | None = None):
    return (src, type.value, label, alignment, "_internal_came_from_package_fn")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    partitions = []
    for src, type, label, alignment, _token in ctx.attrs.partitions:
        if alignment == 0 or (alignment != None and alignment % ctx.attrs.block_size != 0):
            fail("alignment must be a multiple of block size")

        partitions.append({
            "alignment": alignment,
            "name": label,
            "src": src[GptPartitionSource].src,
            "type": type,
        })

    if not partitions:
        fail("must have at least one partition")

    if ctx.attrs.block_size not in (512, 4096):
        fail("invalid block size: {}".format(ctx.attrs.block_size))

    spec_json = ctx.actions.declare_output("spec.json")
    spec = ctx.actions.write_json(
        spec_json,
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
        category = "antlir2_package",
        identifier = "gpt",
    )
    return [DefaultInfo(
        out,
        sub_targets = {
            "spec.json": [DefaultInfo(spec_json)],
        },
    )]

_gpt = rule(
    impl = _impl,
    attrs = {
        "block_size": attrs.int(default = 512, doc = "block size of the gpt layout in bytes"),
        "build_appliance": attrs.option(attrs.exec_dep(providers = [LayerInfo]), default = None),
        "disk_guid": attrs.option(attrs.string(), default = None),
        "labels": attrs.list(attrs.string(), default = []),
        "partitions": attrs.list(
            attrs.tuple(
                attrs.dep(providers = [GptPartitionSource]),
                attrs.enum(PartitionType.values(), default = "linux"),
                attrs.option(attrs.string()),
                attrs.option(attrs.int()),
                attrs.enum(["_internal_came_from_package_fn"]),
            ),
        ),
        "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
    },
)

gpt = rule_with_default_target_platform(_gpt)
