# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The `package.new` rule serializes an `image_layer` target into one or more
files, as described by the specified `format`.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/antlir2/bzl/package:defs.bzl?v2_only", antlir2_package = "package")
load("//antlir/bzl:bash.bzl", "wrap_bash_build_in_common_boilerplate")
load("//antlir/bzl:build_defs.bzl", "buck_genrule", "use_antlir2")
load("//antlir/bzl:loopback_opts.bzl", "normalize_loopback_opts")
load("//antlir/bzl:query.bzl", "layer_deps_query")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "targets_and_outputs_arg_list")

_IMAGE_PACKAGE = "image_package"

_SENDSTREAM_FORMATS = ("sendstream", "sendstream.v2", "sendstream.zst")

def package_new(
        name,
        layer,
        visibility = None,
        # Since `package` produces a real Buck-visible build artifact,
        # "user-facing" is the only sane default.  See comments in
        # `build_defs.bzl` for how this works.
        antlir_rule = "user-facing",
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
        zstd_compression_level = None):
    if use_antlir2():
        antlir2_package.backward_compatible_new(
            name = name,
            layer = layer,
            format = format,
            opts = {
                "compression_level": 3,
            } if format in ("sendstream.v2", "sendstream.zst") else None,
        )
        return

    visibility = visibility or []

    if not format:
        fail("`format` is required for package.new")

    if format in ("ext3", "vfat") and not loopback_opts:
        fail("loopback_opts are required when using format: {}".format(format))

    if subvol_name and format not in _SENDSTREAM_FORMATS:
        fail("subvol_name is only supported for sendstreams")
    if format in _SENDSTREAM_FORMATS and not subvol_name:
        subvol_name = "volume"

    loopback_opts = normalize_loopback_opts(loopback_opts)

    buck_genrule(
        name = name,
        out = "layer." + format,
        type = _IMAGE_PACKAGE,  # For queries
        bash = wrap_bash_build_in_common_boilerplate(
            # We don't need to hold any subvolume lock because we trust
            # that (a) Buck will keep our input JSON alive, and (b) the
            # existence of the JSON will keep the refcount above 1,
            # preventing any concurrent image builds from
            # garbage-collecting the subvolumes.
            bash = '''
            # NB: Using the `location` macro instead of `exe` would
            # cause failures to rebuild on changes to `package-image` in
            # `@mode/dev`, where the rule's "output" is just a symlink.
            # On the other hand, `exe` does not expand to a single file,
            # but rather to a shell snippet, so it's not always what one
            # wants either.
            $(exe {package_image}) \
              --subvolumes-dir "$SUBVOLUMES_DIR" \
              --layer-path $(query_outputs {layer}) \
              --format {format} \
              --output-path "$OUT" \
              {targets_and_outputs} \
              {maybe_loopback_opts} \
              {maybe_subvol_name} \
              {maybe_ba_tgt} \
              {maybe_zstd_compression_level} \
            '''.format(
                format = format,
                layer = layer,
                # We build a list of targets -> outputs using the basic
                # layer_deps_query to ensure that we can always find the
                # build appliance that built the layer in the first place.
                # This build appliance will be the one used to package the
                # layer.
                targets_and_outputs = " ".join(targets_and_outputs_arg_list(
                    name = name,
                    query = layer_deps_query(layer),
                )),
                maybe_loopback_opts = "--loopback-opts {}".format(
                    shell.quote(shape.do_not_cache_me_json(loopback_opts)),
                ) if loopback_opts else "",
                maybe_subvol_name = "--subvol-name {}".format(
                    shell.quote(subvol_name),
                ) if subvol_name else "",
                maybe_ba_tgt = "--ba-tgt {}".format(
                    shell.quote(ba_tgt),
                ) if ba_tgt else "",
                maybe_zstd_compression_level = "--zstd-compression-level {}".format(
                    zstd_compression_level,
                ) if zstd_compression_level else "",
                # Future: When adding support for incremental outputs,
                # use something like this to obtain all the ancestors,
                # so that the packager can verify that the specified
                # base for the incremental computation is indeed an
                # ancestor:
                #     --ancestor-jsons $(query_outputs "attrfilter( \
                #       type, image_layer, deps({layer}))")
                # This could replace `--subvolume-json`, though also
                # specifying it would make `get_subvolume_on_disk_stack`
                # more efficient.
                # NOTE: With the addition of `targets_and_outputs`
                # we now have this ancestor history available.
                package_image = antlir_dep(":package-image"),
            ),
            target_name = name,
        ),
        visibility = visibility,
        labels = ["uses_sudo"] + (labels or []),
        antlir_rule = antlir_rule,
    )
