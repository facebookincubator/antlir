# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Shadowing mountpoints will never be allowed. Additionally, for now:

  - The mountpoint must not exist, and is automatically created as an empty
    directory or file with root:root ownership.  If needed, we may add a flag
    to accept pre-existing empty mountpoints (`remove_paths` is a workaround).
    The motivation for auto-creating the mountpoint is two-fold:
      * This reduces boilerplate in features with `mounts` -- the properties of
        the mountpoint don't matter.
      * This guarantees the mounpoint is empty.

  - Nesting mountpoints is forbidden. If support is ever added, we should make
    the intent to nest very explicit (syntax TBD).

  - All mounts are read-only.

### Implementation Details

A mount target, roughly, is a JSON blob with a "type" string, a "source"
location interpretable by that type, and a "default_mountpoint".  We use
targets as mount sources because:

  - This allows mounts to be materialized, flexibly, at build-time, and allows
    us to provide a cheap "development time" proxy for mounts that might be
    distributed in a more costly way at deployment time.

  - This allows us Buck to cleanly cache mounts fetched from package
    distribution systems -- i.e.  we can use the local Buck cache the same way
    that Docker caches downloaded images.

Adding a mount has two side effects on the `image.layer`:
  - The mount will be materialized in the `buck-image-out` cache of the local
    repo, so your filesystem acts as WYSIWIG.
  - The mount will be recorded in `/.meta/private/mount`.  PLEASE, do not rely
    on this serializaation format for now, it will change.  That's why it's
    "private".

Future: we may need another feature for removing mounts provided by parent
layers.
"""

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:build_defs.bzl", "is_buck2")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")
load(":mount.shape.bzl", "build_source_t", "mount_config_t", "mount_spec_t")

def _feature_host_mount(source, mountpoint, is_directory):
    return mount_spec_t(
        mount_config = mount_config_t(
            build_source = build_source_t(
                source = source,
                type = "host",
            ),
            default_mountpoint = source if mountpoint == None else mountpoint,
            is_directory = is_directory,
        ),
        mountpoint = None,
        target = None,
    )

def feature_host_dir_mount(source, mountpoint = None):
    """
`feature.host_dir_mount("/path/foo")` bind-mounts the host directory
`/path/foo` into the container at `/path/foo`. Another image item must
provide the parent `/path`, but this item will create the mount-point.
    """
    mount_spec = _feature_host_mount(
        source,
        mountpoint,
        is_directory = True,
    )
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(mounts = [mount_spec]),
        antlir2_feature = antlir2_feature.host_mount(
            source = source,
            mountpoint = mountpoint or source,
            is_directory = True,
        ) if is_buck2() else None,
    )

def feature_host_file_mount(source, mountpoint = None):
    """
`feature.host_file_mount("/path/bar", "/baz")` bind-mounts the file `/path/bar`
into the container at `/baz`.
    """
    mount_spec = _feature_host_mount(
        source,
        mountpoint,
        is_directory = False,
    )
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(mounts = [mount_spec]),
        antlir2_feature = antlir2_feature.host_mount(
            source = source,
            mountpoint = mountpoint,
            is_directory = False,
        ) if is_buck2() else None,
    )

def feature_layer_mount(source, mountpoint = None, antlir2_mountpoint = None):
    """
`feature.layer_mount(":other-image-layer")` makes the specified layer available
inside the container available at the "default_mountpoint" provided by the
layer in its config. That fails if the layer lacks a default mountpoint, but
then you can pass an explicit `mountpoint` argument.
    """
    target_tagger = new_target_tagger()
    mount_spec = mount_spec_t(
        mount_config = None,
        mountpoint = mountpoint,
        target = tag_target(target_tagger, source),
    )

    return target_tagger_to_feature(
        target_tagger = target_tagger,
        items = struct(mounts = [mount_spec]),
        antlir2_feature = antlir2_feature.layer_mount(
            source = source,
            mountpoint = antlir2_mountpoint or mountpoint,
        ) if is_buck2() else None,
    )
