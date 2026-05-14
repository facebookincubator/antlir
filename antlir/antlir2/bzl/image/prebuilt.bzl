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
load("//antlir/antlir2/bzl/package:oci.bzl", "OciLayersInfo", "oci_arch")
load("//antlir/antlir2/os:oses.bzl", "OSES")
load("//antlir/bzl:internal_external.bzl", "internal_external")

PrebuiltImageInfo = provider(
    fields = [
        "format",  # format of the image file
        "source",  # source file of the image
    ]
)

def _receive_common_outputs(ctx: AnalysisContext) -> (Artifact, Artifact):
    subvol_symlink = ctx.actions.declare_output("subvol_symlink", has_content_based_path = False)
    facts_db = ctx.actions.declare_output("facts", has_content_based_path = False)
    return subvol_symlink, facts_db

def _layer_providers(ctx: AnalysisContext, subvol_symlink: Artifact, facts_db: Artifact) -> list[Provider]:
    contents = LayerContents(
        subvol_symlink = subvol_symlink,
        subvol_symlink_rootless = ctx.attrs._rootless,
        configured_working_format = ctx.attrs._working_format,
    )
    return [
        LayerInfo(
            label = ctx.label,
            facts_db = facts_db,
            contents = contents,
            features = [],
            mounts = [],
            parent = None,
            flavor = ctx.attrs.flavor,
            phase_contents = [
                (
                    BuildPhase("compile"),
                    contents,
                )
            ],
            supplements = {},
        ),
        DefaultInfo(
            subvol_symlink,
            sub_targets = {
                "debug": [
                    DefaultInfo(
                        sub_targets = {
                            "facts": [DefaultInfo(facts_db)],
                        }
                    )
                ],
            },
        ),
    ]

def _impl(ctx: AnalysisContext) -> list[Provider]:
    format = ctx.attrs.format
    src = ctx.attrs.src
    if format == "sendstream.zst":
        format = "sendstream"
    if format == "sendstream":
        if ctx.attrs.src.basename.endswith("zst"):
            src = ctx.actions.declare_output("uncompressed", has_content_based_path = False)
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
            src = ctx.actions.declare_output("uncompressed", has_content_based_path = False)
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
            src = ctx.actions.declare_output("uncompressed", has_content_based_path = False)
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

    if ctx.attrs.force_root_ownership and format not in ["caf", "tar"]:
        fail("force_root_ownership is not supported for format={}".format(format))

    subvol_symlink, facts_db = _receive_common_outputs(ctx)
    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_receive[RunInfo],
            cmd_args(src, format = "--source={}"),
            cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
            "--force-root-ownership" if ctx.attrs.force_root_ownership else cmd_args(),
            cmd_args(subvol_symlink.as_output(), format = "--output={}"),
            cmd_args(facts_db.as_output(), format = "--facts-db-out={}"),
            cmd_args(ctx.attrs.build_appliance[BuildApplianceInfo].dir, format = "--build-appliance={}"),
            cmd_args(ctx.attrs._package_manager, format = "--package-manager={}"),
            format,
            cmd_args(ctx.attrs._btrfs[RunInfo], format = "--btrfs={}") if format == "sendstream" and ctx.attrs._btrfs else cmd_args(),
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

    return _layer_providers(ctx, subvol_symlink, facts_db)

_prebuilt = rule(
    impl = _impl,
    attrs = {
        "antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
        "antlir2_receive": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_receive:antlir2-receive")),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "force_root_ownership": attrs.bool(default = False),
        "format": attrs.enum(["sendstream.v2", "sendstream", "sendstream.zst", "tar", "caf"]),
        "labels": attrs.list(attrs.string(), default = []),
        "src": attrs.source(doc = "source file of the image"),
        "_btrfs": attrs.option(attrs.exec_dep(), default = None),
        "_rootless": rootless_cfg.is_rootless_attr,
    }
    | cfg_attrs()
    | attrs_selected_by_cfg,
    cfg = layer_cfg,
)

_prebuilt_macro = rule_with_default_target_platform(_prebuilt)

def _oci_impl(ctx: AnalysisContext) -> list[Provider]:
    subvol_symlink, facts_db = _receive_common_outputs(ctx)
    oci_layers_dir = ctx.actions.declare_output("oci_layers", dir = True, has_content_based_path = False)
    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_receive[RunInfo],
            cmd_args(ctx.attrs.src, format = "--source={}"),
            cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
            cmd_args(subvol_symlink.as_output(), format = "--output={}"),
            cmd_args(facts_db.as_output(), format = "--facts-db-out={}"),
            cmd_args(ctx.attrs.build_appliance[BuildApplianceInfo].dir, format = "--build-appliance={}"),
            cmd_args(ctx.attrs._package_manager, format = "--package-manager={}"),
            "oci",
            cmd_args(ctx.attrs.oci_ref, format = "--ref={}") if ctx.attrs.oci_ref else cmd_args(),
            cmd_args(oci_arch(ctx.attrs._selected_target_arch), format = "--arch={}"),
            cmd_args(oci_layers_dir.as_output(), format = "--oci-layers-out={}"),
        ),
        category = "antlir2_prebuilt_layer",
        identifier = "oci",
        # needs to create a local subvolume
        local_only = True,
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = True,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
        error_handler = antlir2_error_handler,
    )

    return _layer_providers(ctx, subvol_symlink, facts_db) + [
        OciLayersInfo(
            layers = [],
            oci_layers_dir = oci_layers_dir,
        ),
    ]

_oci_prebuilt = rule(
    impl = _oci_impl,
    attrs = {
        "antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
        "antlir2_receive": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_receive:antlir2-receive")),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "labels": attrs.list(attrs.string(), default = []),
        "oci_ref": attrs.option(attrs.string(), default = None),
        "src": attrs.source(doc = "source file of the OCI layout"),
        "_rootless": rootless_cfg.is_rootless_attr,
    }
    | cfg_attrs()
    | attrs_selected_by_cfg,
    cfg = layer_cfg,
)

_oci_prebuilt_macro = rule_with_default_target_platform(_oci_prebuilt)

def prebuilt(*args, **kwargs):
    format = kwargs.pop("format")
    rootless = kwargs.pop("rootless", get_antlir2_rootless())

    kwargs["rootless"] = rootless

    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])

    # prebuilt layers are basically useless on their own, so let's just force
    # that an os is configured for them by an rdep
    kwargs.setdefault("compatible_with", [os.select_key for os in OSES])
    kwargs.setdefault(
        "exec_compatible_with",
        [
            "prelude//platforms:may_run_local",
        ],
    )

    if format == "oci":
        _oci_prebuilt_macro(*args, **kwargs)
    else:
        _prebuilt_macro(
            format = format,
            _btrfs = internal_external(
                fb = "fbsource//third-party/btrfs-progs:btrfs",
                oss = None,
            ),
            *args,
            **kwargs,
        )
