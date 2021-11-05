# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The `package.new` rule serializes an `image_layer` target into one or more
files, as described by the specified `format`.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:constants.bzl", "DO_NOT_USE_BUILD_APPLIANCE", "REPO_CFG")
load("//antlir/bzl:image_utils.bzl", "image_utils")
load("//antlir/bzl:loopback_opts.bzl", "normalize_loopback_opts")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")

_IMAGE_PACKAGE = "image_package"

def package_new(
        name,
        layer,
        visibility = None,
        # Since `package` produces a real Buck-visible build artifact,
        # "user-facing" is the only sane default.  See comments in
        # `oss_shim.bzl` for how this works.
        antlir_rule = "user-facing",
        # Build appliance to use when creating packages
        build_appliance = None,
        # The format to use
        # For supported formats, see `--format` here:
        #     buck run //antlir:package-image -- --help
        format = None,
        # Buck `labels` to add to the resulting target; aka `tags` in fbcode.
        labels = None,
        # Opts are required when format == ext3 | vfat | btrfs
        loopback_opts = None):
    visibility = visibility or []
    build_appliance = build_appliance or REPO_CFG.artifact["build_appliance.newest"]

    if not format:
        fail("`format` is required for package.new")

    if format in ("ext3", "vfat") and not loopback_opts:
        fail("loopback_opts are required when using format: {}".format(format))

    loopback_opts = normalize_loopback_opts(loopback_opts)

    buck_genrule(
        name = name,
        out = "layer." + format,
        type = _IMAGE_PACKAGE,  # For queries
        # This is very temporary to work around an FB-internal issue.
        cacheable = False,
        bash = image_utils.wrap_bash_build_in_common_boilerplate(
            self_dependency = antlir_dep("bzl/image/package:new"),
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
              --subvolumes-dir "$subvolumes_dir" \
              --layer-path $(query_outputs {layer}) \
              --format {format} \
              --output-path "$OUT" \
              {maybe_build_appliance} \
              {maybe_loopback_opts}
            '''.format(
                format = format,
                layer = layer,
                maybe_build_appliance = "--build-appliance $(query_outputs {})".format(
                    build_appliance,
                ) if build_appliance != DO_NOT_USE_BUILD_APPLIANCE else "",
                maybe_loopback_opts = "--loopback-opts {}".format(
                    shell.quote(shape.do_not_cache_me_json(loopback_opts)),
                ) if loopback_opts else "",
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
                package_image = antlir_dep(":package-image"),
            ),
            rule_type = _IMAGE_PACKAGE,
            target_name = name,
            volume_min_free_bytes = 0,  # We are not writing to the volume.
        ),
        visibility = visibility,
        labels = labels or [],
        antlir_rule = antlir_rule,
    )
