"""
The `image_package` rule serializes an `image_layer` target into one or more
files, as described by the specified `format`.
"""

load("@bazel_skylib//lib:paths.bzl", "paths")
load(":oss_shim.bzl", "buck_genrule", "get_visibility")
load(":image_utils.bzl", "image_utils")

_IMAGE_PACKAGE = "image_package"

def image_package(
        # Standard naming: <image_layer_name>.<package_format>.
        #
        # For supported formats, see `--format` here:
        #
        #     buck run :package-image -- --help
        #
        # If you are packaging an `image_layer` from a different TARGETS
        # file, then pass `layer =`, and specify whatever name you want.
        name = None,
        # If possible, do not set this. Prefer the standard naming convention.
        layer = None,
        visibility = None,
        writable_subvolume = False,
        seed_device = False):
    visibility = get_visibility(visibility, name)

    local_layer_rule, format = paths.split_extension(name)
    compound_format_specifiers = (
        ".sendstream.zst",
        ".tar.gz",
    )
    for compound_fmt in compound_format_specifiers:
        if name.endswith(compound_fmt):
            local_layer_rule = name[:-len(compound_fmt)]
            format = compound_fmt
            break

    if not format.startswith("."):
        fail(name)
    format = format[1:]
    if "\000" in format or "/" in format:
        fail(repr(name))
    if layer == None:
        layer = ":" + local_layer_rule
    buck_genrule(
        name = name,
        out = "layer." + format,
        type = _IMAGE_PACKAGE,  # For queries
        bash = image_utils.wrap_bash_build_in_common_boilerplate(
            self_dependency = "//fs_image/bzl:image_package",
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
            $(exe //fs_image:package-image) \
              --subvolumes-dir "$subvolumes_dir" \
              --layer-path $(query_outputs {layer}) \
              --format {format} \
              --output-path "$OUT" \
              {rw} \
              {seed} 
            '''.format(
                format = format,
                layer = layer,
                rw = "--writable-subvolume" if writable_subvolume else "",
                seed = "--seed-device" if seed_device else "",
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
            volume_min_free_bytes = 0,  # We are not writing to the volume.
            rule_type = _IMAGE_PACKAGE,
            target_name = name,
        ),
        visibility = visibility,
    )
