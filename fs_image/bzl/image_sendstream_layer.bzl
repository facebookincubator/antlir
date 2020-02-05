load(":compile_image_features.bzl", "compile_image_features")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":image_utils.bzl", "image_utils")
load(":target_tagger.bzl", "image_source_as_target_tagged_dict", "new_target_tagger")

# See the `_image_layer_impl` signature (in `image_layer_utils.bzl`) for all
# other supported kwargs.
def image_sendstream_layer(
        name,
        # `image.source` (see `image_source.bzl`) or path to a target
        # outputting a btrfs send-stream of a subvolume.
        source = None,
        # A struct containing fields accepted by `_build_opts` from
        # `image_layer_compiled.bzl`.
        build_opts = None,
        # Future: Support `parent_layer`.  Mechanistically, applying a
        # send-stream on top of an existing layer is just a regular `btrfs
        # receive`.  However, the rules in the current `receive`
        # implementation for matching the parent to the stream are kind of
        # awkward, and it's not clear whether they are right for us in Buck.
        **image_layer_kwargs):
    target_tagger = new_target_tagger()
    image_layer_utils.image_layer_impl(
        _rule_type = "image_sendstream_layer",
        _layer_name = name,
        _make_subvol_cmd = compile_image_features(
            current_target = image_utils.current_target(name),
            parent_layer = None,
            features = [struct(
                items = struct(
                    receive_sendstreams = [{
                        "source": image_source_as_target_tagged_dict(
                            target_tagger,
                            source,
                        ),
                    }],
                ),
                deps = target_tagger.targets.keys(),
            )],
            build_opts = build_opts,
        ),
        **image_layer_kwargs
    )
