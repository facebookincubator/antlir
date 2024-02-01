# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/package:btrfs.bzl?v2_only", antlir2_BtrfsSubvol = "BtrfsSubvol", antlir2_btrfs = "btrfs")
load("//antlir/bzl:antlir2_shim.bzl", "antlir2_shim")
load("//antlir/bzl:loopback_opts.bzl", "normalize_loopback_opts")
load("//antlir/bzl:structs.bzl", "structs")
load(":btrfs.shape.bzl", "btrfs_opts_t", "btrfs_subvol_t")

def _new_btrfs_subvol(**kwargs):
    return btrfs_subvol_t(
        **kwargs
    )

_btrfs_subvol_api = struct(
    new = _new_btrfs_subvol,
    t = btrfs_subvol_t,
)

def _new_btrfs_opts(subvols, default_subvol = None, loopback_opts = None, **kwargs):
    if default_subvol and not default_subvol.startswith("/"):
        fail("Default subvol must be an absolute path: " + default_subvol)

    loopback_opts = normalize_loopback_opts(loopback_opts)
    if structs.to_dict(loopback_opts).get("size_mb", None) != None:
        fail(
            "The 'size_mb' parameter is not supported for btrfs packages." +
            " Use 'free_mb' instead.",
        )

    return btrfs_opts_t(
        subvols = subvols,
        default_subvol = default_subvol,
        loopback_opts = loopback_opts,
        **kwargs
    )

_btrfs_opts_api = struct(
    new = _new_btrfs_opts,
    subvol = _btrfs_subvol_api,
    t = btrfs_opts_t,
)

def _new_btrfs_shim(
        name,
        # Opts are required
        opts,
        # Buck `labels` to add to the resulting target; aka `tags` in fbcode.
        labels = None,
        visibility = None,
        antlir_rule = "user-facing"):
    if opts.loopback_opts.size_mb:
        fail("size_mb not supported in btrfs")
    if opts.loopback_opts.fat_size:
        fail("fat_size not supported in btrfs")
    opts_kwargs = {
        "compression_level": opts.compression_level,
        "default_subvol": opts.default_subvol,
        "free_mb": opts.free_mb,
        "label": opts.loopback_opts.label,
        "labels": labels,
        "seed_device": opts.seed_device,
        "subvols": {
            subvol_name: antlir2_BtrfsSubvol(
                layer = subvol.layer,
                writable = subvol.writable,
            )
            for subvol_name, subvol in opts.subvols.items()
        },
    }

    if antlir2_shim.upgrade_or_shadow_package(
        antlir2 = None,
        name = name,
        fn = antlir2_btrfs,
        visibility = visibility,
        fake_buck1 = struct(
            fn = antlir2_shim.fake_buck1_target,
            name = name,
        ),
        **opts_kwargs
    ) != "upgrade":
        fail("antlir1 is dead")

btrfs = struct(
    new = _new_btrfs_shim,
    opts = _btrfs_opts_api,
)
