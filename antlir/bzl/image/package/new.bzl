# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The `package.new` rule serializes an `image_layer` target into one or more
files, as described by the specified `format`.
"""

load("//antlir/antlir2/bzl/package:defs.bzl?v2_only", antlir2_package = "package")
load("//antlir/bzl:antlir2_shim.bzl", "antlir2_shim")
load("//antlir/bzl:build_defs.bzl", "get_visibility")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")

_IMAGE_PACKAGE = "image_package"

_SENDSTREAM_FORMATS = ("sendstream", "sendstream.v2", "sendstream.zst")

def package_new(
        name,
        layer,
        visibility = None,
        # Since `package` produces a real Buck-visible build artifact,
        # "user-facing" is the only sane default.  See comments in
        # `build_defs.bzl` for how this works.

        # The format to use
        # For supported formats, see `--format` here:
        #     buck run //antlir:package-image -- --help
        format = None,
        # Buck `labels` to add to the resulting target; aka `tags` in fbcode.
        labels = None,
        # Opts are required when format == ext3 | vfat | btrfs
        loopback_opts = None,
        subvol_name = None,
        ba_tgt = None,
        zstd_compression_level = None,
        antlir2 = None):
    visibility = get_visibility(visibility or [])

    if not format:
        fail("`format` is required for package.new")

    if format in ("ext3", "vfat") and not loopback_opts:
        fail("loopback_opts are required when using format: {}".format(format))

    if subvol_name and format not in _SENDSTREAM_FORMATS:
        fail("subvol_name is only supported for sendstreams")
    if format in _SENDSTREAM_FORMATS and not subvol_name:
        subvol_name = "volume"

    if shape.is_any_instance(loopback_opts):
        opts_kwargs = shape.as_dict_shallow(loopback_opts)
    elif loopback_opts:
        opts_kwargs = structs.to_dict(loopback_opts)
    else:
        opts_kwargs = {}

    if antlir2_shim.upgrade_or_shadow_package(
        antlir2 = antlir2,
        name = name,
        fn = antlir2_shim.getattr_buck2(antlir2_package, "backward_compatible_new"),
        layer = layer,
        format = format,
        visibility = visibility,
        **opts_kwargs
    ) != "upgrade":
        fail("antlir1 is dead")
