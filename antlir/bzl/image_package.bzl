# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The `image_package` rule serializes an `image_layer` target into one or more
files, as described by the specified `format`.
"""

load("@bazel_skylib//lib:paths.bzl", "paths")
load(":constants.bzl", "DO_NOT_USE_BUILD_APPLIANCE", "REPO_CFG")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":image_utils.bzl", "image_utils")
load(":oss_shim.bzl", "buck_genrule")

_IMAGE_PACKAGE = "image_package"

def image_package(
        name,
        layer,
        visibility = None,
        writable_subvolume = False,
        seed_device = False,
        set_default_subvol = False,
        # Since `image.package` produces a real Buck-visible build artifact,
        # "user-facing" is the only sane default.  See comments in
        # `oss_shim.bzl` for how this works.
        antlir_rule = "user-facing",
        # Build appliance to use when creating packages
        build_appliance = None,
        # The format to use
        # For supported formats, see `--format` here:
        #     buck run //antlir:package-image -- --help
        format = None,
        # Size of the target image in MiB
        # This is required when format is vfat/ext3 and optional for btrfs
        size_mb = None,
        # Also for vfat/ext3, but optional
        label = None,
        # This flag will ensure that the resulting btrfs loopback image
        # is optimized in opt mode.  This is provided to toggle this
        # behavior in certain cases where it's not desired, such as the
        # building of the vmtest test binary image.
        # This will get rolled up into the `loopback_opts` changes that
        # are coming on D28591961
        optimization = True):
    visibility = visibility or []
    build_appliance = build_appliance or flavor_helpers.default_flavor_build_appliance

    if not format:
        fail("`format` is required for image.package")

    buck_genrule(
        name = name,
        out = "layer." + format,
        type = _IMAGE_PACKAGE,  # For queries
        # This is very temporary to work around an FB-internal issue.
        cacheable = False,
        bash = image_utils.wrap_bash_build_in_common_boilerplate(
            self_dependency = "//antlir/bzl:image_package",
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
            $(exe //antlir:package-image) \
              --subvolumes-dir "$subvolumes_dir" \
              --layer-path $(query_outputs {layer}) \
              --format {format} \
              {maybe_size_mb} \
              {maybe_label} \
              --output-path "$OUT" \
              {maybe_build_appliance} \
              {rw} \
              {seed} \
              {set_default} \
              {multi_pass_size_minimization}
            '''.format(
                format = format,
                maybe_size_mb = "--size-mb {}".format(size_mb) if size_mb else "",
                maybe_label = "--volume-label {}".format(label) if label else "",
                layer = layer,
                maybe_build_appliance = "--build-appliance $(query_outputs {})".format(
                    build_appliance,
                ) if build_appliance != DO_NOT_USE_BUILD_APPLIANCE else "",
                rw = "--writable-subvolume" if writable_subvolume else "",
                seed = "--seed-device" if seed_device else "",
                set_default = "--set-default-subvol" if set_default_subvol else "",
                multi_pass_size_minimization = "--multi-pass-size-minimization" if (
                    (not REPO_CFG.artifacts_require_repo and optimization) and not size_mb
                ) else "",
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
            ),
            rule_type = _IMAGE_PACKAGE,
            target_name = name,
            volume_min_free_bytes = 0,  # We are not writing to the volume.
        ),
        visibility = visibility,
        antlir_rule = antlir_rule,
    )
