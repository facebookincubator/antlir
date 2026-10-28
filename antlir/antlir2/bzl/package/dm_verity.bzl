# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":disk_image.bzl", "DiskImage")
load(":dm_verity_info.bzl", "DmVerityInfo")
load(":gpt.bzl", "GptPartitionSource")

_DEFAULT_SALT = "0000000000000000000000000000000000000000000000000000000000000000"
_DEFAULT_UUID = "00000000-0000-4000-8000-000000000000"

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.data_block_size not in (512, 1024, 2048, 4096):
        fail("data_block_size must be one of 512, 1024, 2048, 4096")
    if ctx.attrs.hash_block_size not in (512, 1024, 2048, 4096):
        fail("hash_block_size must be one of 512, 1024, 2048, 4096")

    src = ctx.attrs.src[GptPartitionSource].src

    out = ctx.actions.declare_output("package.dm_verity")
    root_hash = ctx.actions.declare_output("root_hash.txt")
    hash_image = ctx.actions.declare_output("hash.img")

    spec = ctx.actions.write_json(
        "spec.json",
        {
            "dm_verity": {
                "data_block_size": ctx.attrs.data_block_size,
                "hash_algorithm": ctx.attrs.hash_algorithm,
                "hash_block_size": ctx.attrs.hash_block_size,
                "hash_image": hash_image.as_output(),
                "root_hash": root_hash.as_output(),
                "salt": ctx.attrs.salt,
                "src": src,
                "uuid": ctx.attrs.uuid,
            },
        },
        with_inputs = True,
        has_content_based_path = False,
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(ctx.attrs._working_format, format = "--working-format={}"),
            cmd_args(out.as_output(), format = "--out={}"),
        ),
        category = "antlir2_package",
        identifier = "dm_verity",
    )

    return [
        DefaultInfo(
            out,
            sub_targets = {
                "hash": [DefaultInfo(hash_image)],
                "root_hash": [DefaultInfo(root_hash)],
                "src": ctx.attrs.src.providers,
            },
        ),
        DmVerityInfo(
            image = out,
            root_hash = root_hash,
            hash_image = hash_image,
            data_image = src,
            data_block_size = ctx.attrs.data_block_size,
            hash_block_size = ctx.attrs.hash_block_size,
            hash_algorithm = ctx.attrs.hash_algorithm,
            salt = ctx.attrs.salt,
        ),
        GptPartitionSource(src = out),
        DiskImage(src = out),
    ]

_dm_verity_attrs = {
    "data_block_size": attrs.int(default = 4096, doc = "Size in bytes of each data block hashed by dm-verity"),
    "hash_algorithm": attrs.enum(["sha256", "sha512"], default = "sha256", doc = "Hash algorithm for the hash tree"),
    "hash_block_size": attrs.int(default = 4096, doc = "Size in bytes of each hash block in the hash tree"),
    "labels": attrs.list(attrs.string(), default = []),
    "salt": attrs.string(default = _DEFAULT_SALT, doc = "Hex-encoded salt for hashing. Defaults to 32 zero bytes for reproducible builds."),
    "src": attrs.dep(providers = [GptPartitionSource], doc = "Package to wrap with dm-verity (e.g. package.ext4, package.btrfs). Must provide GptPartitionSource."),
    "uuid": attrs.string(default = _DEFAULT_UUID, doc = "UUID stored in the verity superblock"),
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
}

dm_verity_rule = anon_rule(
    impl = _impl,
    attrs = _dm_verity_attrs,
    artifact_promise_mappings = {
        "hash_image": lambda x: x[DmVerityInfo].hash_image,
        "image": lambda x: x[DmVerityInfo].image,
        "root_hash": lambda x: x[DmVerityInfo].root_hash,
    },
)

dm_verity = rule_with_default_target_platform(dm_verity_rule)
