# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_error_handler:handler.bzl", "antlir2_error_handler")
load("//antlir/antlir2/antlir2_overlayfs:overlayfs.bzl", "get_antlir2_use_overlayfs")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "FlavorInfo", "LayerContents", "LayerInfo")
load("//antlir/bzl:build_defs.bzl", "internal_external")
load(":facts.bzl", "facts")

PrebuiltImageInfo = provider(fields = [
    "format",  # format of the image file
    "source",  # source file of the image
])

def _impl(ctx: AnalysisContext) -> list[Provider]:
    format = ctx.attrs.format
    src = ctx.attrs.src
    if format == "sendstream.zst":
        format = "sendstream"
    if format == "sendstream":
        if ctx.attrs.src.basename.endswith("zst"):
            src = ctx.actions.declare_output("uncompressed")
            ctx.actions.run(
                cmd_args(
                    "zstd",
                    "-d",
                    "-o",
                    src.as_output(),
                    ctx.attrs.src,
                ),
                category = "decompress",
                # we absolutely need the end result locally to `btrfs receive`
                # it, and these images are often huge and spend a ton of time
                # uploading and downloading giant blobs to/from RE
                local_only = True,
            )
    elif format == "sendstream.v2":
        # antlir2-receive treats them the same
        format = "sendstream"

    if format == "tar":
        if ctx.attrs.src.basename.endswith("gz"):
            src = ctx.actions.declare_output("uncompressed")
            ctx.actions.run(
                cmd_args(
                    "bash",
                    "-e",
                    "-c",
                    cmd_args(
                        "zcat",
                        ctx.attrs.src,
                        cmd_args(src.as_output(), format = "> {}"),
                        delimiter = " ",
                    ),
                ),
                category = "decompress",
                # we're going to need it to be locally available to extract it
                # into an image, but it *can* be run remotely
                prefer_local = True,
            )
        if ctx.attrs.src.basename.endswith("zst"):
            src = ctx.actions.declare_output("uncompressed")
            ctx.actions.run(
                cmd_args(
                    "zstd",
                    "-d",
                    ctx.attrs.src,
                    "-o",
                    src.as_output(),
                ),
                category = "decompress",
                # we're going to need it to be locally available to extract it
                # into an image, but it *can* be run remotely
                prefer_local = True,
            )

    subvol_symlink = ctx.actions.declare_output("subvol_symlink")
    ctx.actions.run(
        cmd_args(
            # this usually requires privileged btrfs operations
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_receive[RunInfo],
            "--working-dir=antlir2-out",
            cmd_args(format, format = "--format={}"),
            cmd_args(ctx.attrs._btrfs[RunInfo], format = "--btrfs={}") if format == "sendstream" and ctx.attrs._btrfs else cmd_args(),
            cmd_args(src, format = "--source={}"),
            cmd_args(subvol_symlink.as_output(), format = "--output={}"),
            cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
            cmd_args("--working-format=overlayfs") if ctx.attrs._overlayfs else cmd_args(),
        ),
        category = "antlir2_prebuilt_layer",
        identifier = format,
        # needs local subvolumes
        local_only = not ctx.attrs._overlayfs,
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = not ctx.attrs._overlayfs,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
        error_handler = antlir2_error_handler,
    )

    contents = LayerContents(subvol_symlink = subvol_symlink)

    facts_db = facts.new_facts_db(
        actions = ctx.actions,
        layer = contents,
        parent_facts_db = None,
        build_appliance = ctx.attrs.flavor[FlavorInfo].default_build_appliance[BuildApplianceInfo] if ctx.attrs.flavor and ctx.attrs.flavor[FlavorInfo].default_build_appliance else None,
        new_facts_db = ctx.attrs._new_facts_db[RunInfo],
        phase = None,
        rootless = ctx.attrs._rootless,
    )

    return [
        LayerInfo(
            label = ctx.label,
            facts_db = facts_db,
            contents = contents,
            mounts = [],
            flavor = ctx.attrs.flavor,
        ),
        DefaultInfo(subvol_symlink, sub_targets = {
            "debug": [DefaultInfo(sub_targets = {
                "facts": [DefaultInfo(facts_db)],
            })],
        }),
    ]

_prebuilt = rule(
    impl = _impl,
    attrs = {
        "antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
        "antlir2_receive": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_receive:antlir2-receive")),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "format": attrs.enum(["cas_dir", "sendstream.v2", "sendstream", "sendstream.zst", "tar", "caf"]),
        "labels": attrs.list(attrs.string(), default = []),
        "src": attrs.source(doc = "source file of the image"),
        "_btrfs": attrs.option(attrs.exec_dep()),
        "_new_facts_db": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_facts:new-facts-db"),
        "_overlayfs": attrs.bool(default = False),
        "_rootless": attrs.default_only(attrs.bool(default = select({
            "DEFAULT": False,
            "antlir//antlir/antlir2/antlir2_rootless:rooted": False,
            "antlir//antlir/antlir2/antlir2_rootless:rootless": True,
        }))),
    } | rootless_cfg.attrs,
    cfg = rootless_cfg.rule_cfg,
)

_prebuilt_macro = rule_with_default_target_platform(_prebuilt)

def prebuilt(*args, **kwargs):
    rootless = kwargs.pop("rootless", get_antlir2_rootless())

    if get_antlir2_use_overlayfs():
        kwargs["_overlayfs"] = True
        rootless = True

    kwargs["rootless"] = rootless

    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])

    kwargs["_btrfs"] = internal_external(
        fb = "fbsource//third-party/btrfs-progs:btrfs",
        oss = None,
    )

    _prebuilt_macro(
        *args,
        **kwargs
    )
