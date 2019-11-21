"""
USE WITH CARE -- this was added to aid in implementing `released_layer`.
Most people should avoid aliases, and should instead use an absolute target
path to refer to the original layer.  If you think you need this, talk to
the `fs_image` team.

The output of this target is meant to be indistinguishable from the source
layer.  Both Buck outputs share the same `buck-image-out` subvolume,
minimizing space usage.  This target increments the refcount of the
subvolume in `buck-image-out`, ensuring that it will live on as long any
single reference exists.

Take care: the `mountconfig.json` field `build_source` will point at the
ORIGINAL layer, which can be unexpected for the consumer.  At present, I see
no reason to rewrite this configuration, but this can be revised in the
future.

Of course, there are some differences between the targets from the point of
view of Buck:
  - They have different paths -- that's the point!
  - They may have different visibility settings.
  - Their "type" attribute will differ.
"""

load(":oss_shim.bzl", "buck_genrule", "get_visibility")

def image_layer_alias(name, layer, visibility = None):
    # IMPORTANT: If you touch this genrule, update `_image_layer_impl`.
    buck_genrule(
        name = name,
        out = "layer",
        # Caveats:
        #   - This lacks a "self-dependency" on the `fake_macro_library`
        #     because hardlinks have the property of always being in sync.
        #   - This will break if some clever person adds dotfiles.
        #     In that case, check out bash's `GLOBIGNORE` and `dotglob`.
        bash = '''
        set -ue -o pipefail
        mkdir "$OUT"
        for f in $(location {layer})/* ; do
            ln "$f" "$OUT"/
        done
        '''.format(layer = layer),
        cacheable = False,
        type = "image_layer_alias",
        visibility = get_visibility(visibility, name),
    )
