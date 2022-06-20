# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:shape.bzl", "shape")
load(":image_source.shape.bzl", "image_source_t")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":structs.bzl", "structs")

# Note to users: all callsites accepting `image.source` objects also accept
# plain strings, which are interpreted as `image.source(<the string>)`.
def _image_source_impl(
        # Buck target outputting file or directory, conflicts with `layer`.
        #
        # You may also pass a relative path inside the repo, so long as it
        # does not contain `:` or `../`.  In that case, an internal
        # `export_file` target will automatically be created and used.
        #
        # Can be combined with `path` and `content_hash`.
        #
        # Internal note: If `source` is a struct, it is interpreted as an
        # already-constructed `image.source`.  Implementers of rules that
        # accept `image.source` should always call `image.source(input_src)`
        # to get easy input validation, and to accept `"//target:path"` to
        # mean `image.source("//target:path")`.
        source = None,
        # `image.layer` target, conflicts w/ `source`. Combines with `path`
        # and `content_hash`.
        layer = None,
        # Relative path within `source` or `layer`.  Deliberately not
        # supported for `generator` because it's grossly inefficient to
        # generate more than you need, and then to grab just one file.  The
        # reason we allow it for `source` and `layer` is that it's at least
        # plausible that other parts of those targets' output get used
        # elsewhere.  In contrast, a generator's output is ephemeral to a
        # specific image build.
        path = None,
        # `generator` is a path to an executable target, which will run
        # every time a layer item including this `image.source` is built.
        #
        # Executing the target must generate one deterministic file.  The
        # script's contract is:
        #   - Its arguments are the strings from `generator_args`, followed
        #     by one last argument that is a path to an `image.layer`-
        #     provided temporary directory, into which the generator must
        #     write its file.
        #   - The generator must print the filename of its output, followed
        #     by a single newline, to stdout.  The filename MUST be relative
        #     to the provided temporary directory.
        #   - The file's contents must match `content_hash`, see below.
        #
        # In deciding between `source` / `layer` and `generator`, you are
        # trading off disk space in the Buck cache for the resources (e.g.
        # latency, CPU usage, or network usage) needed to re-generate the
        # file.  For example, using `generator*` is a good choice when it
        # simply performs a download from a fast immutable blob store.
        #
        # Note that a single script can potentially be used both as a
        # generator, and to produce cached artifacts, see how the compiler
        # test `TARGETS` uses `hello_world_tar_generator.sh` in a genrule.
        #
        # Posssible enhancements:
        #   - It's probably reasonable for this to also be able to output
        #     a directory instead of a file. Support this when needed.
        #   - If useful, the compiler could cache and reuse the output
        #     of a generator if it occurs multiple times within a single
        #     layer's build -- this is currently not implemented, but would
        #     not be hard.
        generator = None,
        # Optional list of strings, requires `generator` to be set.
        generator_args = None,
        # Required when `generator` is set, optional when `source` or
        # `layer` is set.  A string of the form `<python hashlib algo>:<hex
        # digest>`, which is asserted to be the hash of the content of the
        # source file.
        content_hash = None):
    if int(bool(source)) + int(bool(layer)) + int(bool(generator)) != 1:
        fail("Exactly one of `source`, `layer`, `generator` must be set")
    if generator_args and not generator:
        fail("`generator_args` require `generator`")

    # The current most important use-case for generators is to pull down
    # known-hash packages from a network store.  There, using a `generator`
    # is important for reducing disk usage as described above.  In this
    # case, we don't want to fully trust the bits received via the network,
    # so hash validation is mandatory.
    #
    # For use-cases where the hash is not easy to hardcode in the TARGETS
    # file, there is an escape hatch -- a user can write a Buck genrule
    # instead of a generator, and use the `source` field, which does NOT
    # require hash validation.  The rationale for making hash validation
    # optional for `source` is that in this case, Buck is responsible for
    # ensuring repo-hermeticity, and should (in time) use a combination of
    # sandboxing and logging to eliminate non- repo-hermetic rules.
    if generator and not content_hash:
        fail(
            "To ensure that generated `image.source`s are repo-hermetic, you " +
            'must pass `content_hash = "algorithm:hexdigest"` (checked via Python ' +
            "hashlib)",
        )

    return image_source_t(
        source = maybe_export_file(source),
        layer = layer,
        path = path,
        generator = generator,
        generator_args = tuple(generator_args or []),
        content_hash = content_hash,
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
