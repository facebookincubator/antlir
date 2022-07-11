# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load(":image_layer_runtime.bzl", "add_runtime_targets")

"""
USE WITH CARE -- this was added to aid in implementing `released_layer`.
Most people should avoid aliases, and should instead use an absolute target
path to refer to the original layer.  If you think you need this, talk to
the `antlir` team.

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

def image_layer_alias(name, layer, runtime = None, visibility = None):
    visibility = visibility or []

    # IMPORTANT: If you touch this genrule, update `_image_layer_impl`.
    buck_genrule(
        name = name,
        # This should definitely not count towards CI dependency distance
        # between sources & build nodes.
        antlir_rule = "user-internal",
        # Caveats:
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
        visibility = visibility,
    )

    add_runtime_targets(name, runtime)
