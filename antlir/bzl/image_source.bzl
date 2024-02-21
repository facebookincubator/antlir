# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# TODO(T139523690) remove this entirely on buck2

load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load(":image_source.shape.bzl", "image_source_t")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")

# Note to users: all callsites accepting `image.source` objects also accept
# plain strings, which are interpreted as `image.source(<the string>)`.
def _image_source_impl(
        # Buck target outputting file or directory, conflicts with `layer`.
        #
        # You may also pass a relative path inside the repo, so long as it
        # does not contain `:` or `../`.  In that case, an internal
        # `export_file` target will automatically be created and used.
        #
        # Can be combined with `path`.
        #
        # Internal note: If `source` is a struct, it is interpreted as an
        # already-constructed `image.source`.  Implementers of rules that
        # accept `image.source` should always call `image.source(input_src)`
        # to get easy input validation, and to accept `"//target:path"` to
        # mean `image.source("//target:path")`.
        source = None,
        # `image.layer` target, conflicts w/ `source`. Can be combined with
        # `path`.
        layer = None,
        # Relative path within `source` or `layer`.  Ideally the source would
        # only have the one thing that is needed, but we allow `path` to extract
        # an individual output since it's plausible that other parts of those
        # targets' output get used elsewhere.
        path = None):
    return image_source_t(
        layer = layer,
        path = path,
        source = maybe_export_file(source),
    )

# `_image_source_impl` documents the function signature.  It is intentional
# that arguments besides `source` are keyword-only.
def image_source_shape(source = None, **kwargs):
    if source == None or types.is_string(source):
        return _image_source_impl(source = source, **kwargs)
    if kwargs:
        fail("Got struct source {} with other args".format(source))
    if shape.is_instance(source, image_source_t):
        # The shape is private to this .bzl file, so barring misuse of
        # `.__shape__`, we know this has already been validated.
        return source
    return _image_source_impl(**structs.to_dict(source))

def image_source(source = None, **kwargs):
    return image_source_shape(source, **kwargs)

def image_source_to_buck2_src(source):
    if types.is_string(source):
        return source
    if source.layer:
        fail("layer source not allowed")
    if not source.path:
        return source.source
    else:
        name = "export-src-{}/{}".format(
            source.source.replace(":", "").replace("/", "_"),
            source.path.replace(":", "").replace("/", "_"),
        )
        if not native.rule_exists(name):
            buck_genrule(
                name = name,
                out = paths.basename(source.path),
                cmd = "cp -r --reflink=auto $(location {})/{} $OUT".format(source.source, source.path),
            )
        return normalize_target(":" + name)
