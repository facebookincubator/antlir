# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_error_handler:handler.bzl", "antlir2_error_handler")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "FlavorInfo", "LayerContents", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/os:oses.bzl", "OSES")
load("//antlir/bzl:internal_external.bzl", "internal_external")

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
    facts_db = ctx.actions.declare_output("facts")
    ctx.actions.run(
        cmd_args(
            # this usually requires privileged btrfs operations
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_receive[RunInfo],
            cmd_args(format, format = "--format={}"),
            cmd_args(ctx.attrs._btrfs[RunInfo], format = "--btrfs={}") if format == "sendstream" and ctx.attrs._btrfs else cmd_args(),
            cmd_args(src, format = "--source={}"),
            cmd_args(subvol_symlink.as_output(), format = "--output={}"),
            cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
            cmd_args(facts_db.as_output(), format = "--facts-db-out={}"),
            cmd_args(ctx.attrs.build_appliance[BuildApplianceInfo].dir, format = "--build-appliance={}"),
        ),
        category = "antlir2_prebuilt_layer",
        identifier = format,
        # needs to create a local subvolume
        local_only = True,
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = True,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
        error_handler = antlir2_error_handler,
    )

    contents = LayerContents(subvol_symlink = subvol_symlink)

    return [
        LayerInfo(
            label = ctx.label,
            facts_db = facts_db,
            contents = contents,
            mounts = [],
            flavor = ctx.attrs.flavor,
            phase_contents = [(
                BuildPhase("compile"),
                contents,
            )],
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
        "format": attrs.enum(["sendstream.v2", "sendstream", "sendstream.zst", "tar", "caf"]),
        "labels": attrs.list(attrs.string(), default = []),
        "src": attrs.source(doc = "source file of the image"),
        "_btrfs": attrs.option(attrs.exec_dep(), default = None),
        "_rootless": rootless_cfg.is_rootless_attr,
    } | cfg_attrs() | attrs_selected_by_cfg(),
    cfg = layer_cfg,
)

_prebuilt_macro = rule_with_default_target_platform(_prebuilt)

def prebuilt(*args, **kwargs):
    rootless = kwargs.pop("rootless", get_antlir2_rootless())

    kwargs["rootless"] = rootless

    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])

    # prebuilt layers are basically useless on their own, so let's just force
    # that an os is configured for them by an rdep
    kwargs.setdefault("compatible_with", [os.select_key for os in OSES])

    _prebuilt_macro(
        _btrfs = internal_external(
            fb = "fbsource//third-party/btrfs-progs:btrfs",
            oss = None,
        ),
        exec_compatible_with = [
            # This rule action already has `local_only=True` actions, so make
            # sure we pick an exec platform that can run locally.
            "prelude//platforms:may_run_local",
        ],
        *args,
        **kwargs
    )
