# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":disk_image.bzl", "DiskImage")
load(":dm_verity_info.bzl", "DmVerityInfo")

GptPartitionSource = provider(fields = ["src"])

PartitionType = enum("linux", "linux-root-x64", "linux-root-arm64", "linux-root-x64-verity", "linux-root-arm64-verity", "esp")

TargetArchLinuxRootPartitionType = select({
    "ovr_config//cpu:arm64": PartitionType("linux-root-arm64"),
    "ovr_config//cpu:x86_64": PartitionType("linux-root-x64"),
})

TargetArchLinuxRootVerityPartitionType = select({
    "ovr_config//cpu:arm64": PartitionType("linux-root-arm64-verity"),
    "ovr_config//cpu:x86_64": PartitionType("linux-root-x64-verity"),
})

# systemd Discoverable Partitions: a verity hash partition has the
# arch-specific root-verity type and shares its PARTUUID with the data
# (root) partition it protects.
_VERITY_TYPE_FOR_ROOT_TYPE = {
    "linux-root-arm64": "linux-root-arm64-verity",
    "linux-root-x64": "linux-root-x64-verity",
}

PartitionFlags = enum("grow_file_system")

def Partition(
    src: str,
    type: PartitionType | Select = PartitionType("linux"),
    flags: list[PartitionFlags] | Select | None = None,
    label: str | None = None,
    alignment: int | None = None,
    uuid: str | None = None,
):
    return (
        src,
        selects.apply(type, lambda t: t.value),
        selects.apply(flags, lambda flags: [f.value for f in flags] if flags else []),
        label,
        alignment,
        uuid,
        "_internal_came_from_package_fn",
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    partitions = []
    for src, type, flags, label, alignment, uuid, _token in ctx.attrs.partitions:
        if alignment == 0 or (alignment != None and alignment % ctx.attrs.block_size != 0):
            fail("alignment must be a multiple of block size")

        if DmVerityInfo in src:
            # systemd Discoverable Partitions layout: do not put the
            # combined dm-verity image in one partition. Split it into the
            # data partition (as requested) plus a hash partition right
            # after it, typed as the matching root-verity partition and
            # sharing the data partition's PARTUUID. The packager pairs
            # them up (see `verity_pair`) when no explicit uuid is given.
            verity_type = _VERITY_TYPE_FOR_ROOT_TYPE.get(type)
            if verity_type == None:
                fail(
                    "dm-verity partition ({}) must use a root partition type (linux-root-x64 or linux-root-arm64, e.g. TargetArchLinuxRootPartitionType), not '{}', so a matching verity partition can be created".format(
                        src.label, type
                    )
                )
            info = src[DmVerityInfo]
            pair = "verity-{}".format(len(partitions))
            partitions.append({
                "alignment": alignment,
                "flags": " | ".join([f.upper() for f in flags]),
                "name": label,
                "src": info.data_image,
                "type": type,
                "uuid": uuid,
                "verity_pair": pair,
            })
            partitions.append({
                "alignment": alignment,
                "flags": "",
                "name": (label + "-verity") if label else "verity",
                "src": info.hash_image,
                "type": verity_type,
                "uuid": uuid,
                "verity_pair": pair,
            })
        else:
            partitions.append({
                "alignment": alignment,
                "flags": " | ".join([f.upper() for f in flags]),
                "name": label,
                "src": src[GptPartitionSource].src,
                "type": type,
                "uuid": uuid,
            })

    if not partitions:
        fail("must have at least one partition")

    if ctx.attrs.block_size not in (512, 4096):
        fail("invalid block size: {}".format(ctx.attrs.block_size))

    # If we're building a fbpkg, put its uuid in the gpt header
    disk_guid = ctx.attrs.disk_guid or native.read_root_config("build_info", "package_version")

    spec_json = ctx.actions.declare_output("spec.json", has_content_based_path = False)
    spec = ctx.actions.write_json(
        spec_json,
        {
            "gpt": {
                "block_size": str(ctx.attrs.block_size),
                "disk_guid": disk_guid,
                "partitions": partitions,
            },
        },
        with_inputs = True,
    )
    out = ctx.actions.declare_output("package.gpt", has_content_based_path = False)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(ctx.attrs._working_format, format = "--working-format={}"),
            cmd_args(out.as_output(), format = "--out={}"),
        ),
        category = "antlir2_package",
        identifier = "gpt",
    )
    return [
        DefaultInfo(
            out,
            sub_targets = {
                "spec.json": [DefaultInfo(spec_json)],
            },
        ),
        DiskImage(src = out),
    ]

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
                attrs.list(
                    attrs.enum(PartitionFlags.values()),
                    default = [],
                ),
                attrs.option(attrs.string()),
                attrs.option(attrs.int()),
                attrs.option(attrs.string()),
                attrs.enum(["_internal_came_from_package_fn"]),
            ),
        ),
        "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
        "_working_format": attrs.default_only(
            attrs.string(
                default = select({
                    "DEFAULT": "btrfs",
                    "antlir//antlir/antlir2/cfg:working_format[btrfs]": "btrfs",
                    "antlir//antlir/antlir2/cfg:working_format[cad-stack]": "cad-stack",
                }),
            )
        ),
    },
)

gpt = rule_with_default_target_platform(_gpt)
