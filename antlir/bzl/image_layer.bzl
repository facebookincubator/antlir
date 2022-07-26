# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
An `image.layer` is a set of `feature` with some additional parameters.  Its
purpose to materialize those `feature`s as a btrfs subvolume in the
per-repo `buck-image-out/volume/targets`.

We call the subvolume a "layer" because it can be built on top of a snapshot
of its `parent_layer`, and thus can be represented as a btrfs send-stream for
more efficient storage & distribution.

The Buck output of an `image.layer` target is a JSON file with information
on how to find the resulting layer in the per-repo
`buck-image-out/volume/targets`.  See `SubvolumeOnDisk.to_json_file`.

## Implementation notes

The implementation of this converter deliberately minimizes the amount of
business logic in its command.  The converter must include **only** our
interactions with the buck target graph.  Everything else should be
delegated to subcommands.

### Command

In composing the `bash` command, our core maxim is: make it a hermetic
function of the converter's inputs -- do not read data from disk, do not
insert disk paths into the command, do not do anything that might cause the
bytes of the command to vary between machines or between runs.  To achieve
this, we use Buck macros to resolve all paths, including those to helper
scripts.  We rely on environment variables or pipes to pass data between the
helper scripts.

Another reason to keep this converter minimal is that `buck test` cannot
make assertions about targets that fail to build.  Since we only have the
ability to test the "good" targets, it behooves us to put most logic in
external scripts, so that we can unit-test its successes **and** failures
thoroughly.

### Output

We mark `image.layer` uncacheable, because there's no easy way to teach Buck
to serialize a btrfs subvolume (for that, we have `package.new`).

That said, we should still follow best practices to avoid problems if e.g.
the user renames their repo, or similar.  These practices include:
  - The output JSON must store no absolute paths.
  - Store Buck target paths instead of paths into the output directory.

### Dependency resolution

An `image.layer` consumes a set of `feature` outputs to decide what to put into
the btrfs subvolume.  These outputs are actually just JSON files that
reference other targets, and do not contain the data to be written into the
image.

Therefore, `image.layer` has to explicitly tell buck that it needs all
direct dependencies of its `feature`s to be present on disk -- see our
`attrfilter` queries below.  Without this, Buck would merrily fetch the just
the `feature` JSONs from its cache, and not provide us with any of the
buid artifacts that comprise the image.

We do NOT need the direct dependencies of the parent layer's features,
because we treat the parent layer as a black box -- whatever it has laid
down in the image, that's what it provides (and we don't care about how).
The consequences of this information hiding are:

  - Better Buck cache efficiency -- we don't have to download
    the dependencies of the ancestor layers' features. Doing that would be
    wasteful, since those bits are redundant with what's in the parent.

  - Ability to use genrule image layers / apply non-pure post-processing to
    a layer.  In terms of engineering, both of these non-pure approaches are
    a terrible idea and a maintainability headache, but they do provide a
    useful bridge for transitioning to Buck image builds from legacy
    imperative systems.

  - The image compiler needs a litte extra code to walk the parent layer and
    determine what it provides.

  - We cannot have "unobservable" dependencies between features.  Since
    feature dependencies are expected to routinely cross layer boundaries,
    feature implementations are forced only to depend on data that can be
    inferred from the filesystem -- since this is all that the parent layer
    implementation can do.  NB: This is easy to relax in the future by
    writing a manifest with additional metadata into each layer, and using
    that metadata during compilation.
"""

load(":compile_image_features.bzl", "compile_image_features")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":target_helpers.bzl", "normalize_target")

def image_layer(
        name,
        parent_layer = None,
        features = None,
        extra_deps = None,
        flavor = None,
        flavor_config_override = None,
        antlir_rule = "user-internal",
        **image_layer_kwargs):
    """
    Arguments

    - `parent_layer`: The name of another `image_layer` target, on
    top of which the current layer will install its features.
    - `features`: List of `feature` target paths and/or
    nameless structs from `feature.new`.
    - `flavor`: Picks default build options for the layer, including
    `build_appliance`, RPM installer, and others. See `flavor_helpers.bzl`
    for details.
    - `flavor_config_override`: A struct that can override the default
    values fetched from `REPO_CFG[flavor].flavor_to_config`.
    - `mount_config`: Specifies how this layer is mounted in the
    `mounts` field of a `feature` of a parent layer. See
    the field in `_image_layer_impl` in `image_layer_utils.bzl`
    - `runtime`: A list of desired helper buck targets to be emitted.
    `container` is always included in the list by default.
    See the field in `_image_layer_impl` in `image_layer_utils.bzl` and the
    [docs](/docs/tutorials/helper-buck-targets#imagelayer) for the list of
    possible helpers, their respective behaviours, and how to invoke them.
    """

    image_layer_utils.image_layer_impl(
        _rule_type = "image_layer",
        _layer_name = name,
        # Build a new layer. It may be empty.
        _make_subvol_cmd = compile_image_features(
            name = name,
            current_target = normalize_target(":" + name),
            parent_layer = parent_layer,
            features = features,
            extra_deps = extra_deps,
            flavor = flavor,
            flavor_config_override = flavor_config_override,
        ),
        antlir_rule = antlir_rule,
        **image_layer_kwargs
    )
