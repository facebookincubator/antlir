"""
Our continuous integration system might run different build steps in
different sandboxes, so the intermediate outputs of `image_feature`s
must be cacheable by Buck.  In particular, they must not contain
absolute paths to targets.

However, to build a dependent `image_layer`, we will need to invoke the
image compiler with the absolute paths of the outputs that will comprise
the image.

Therefore, we need to (a) record all the targets, for which the image
compiler will need absolute paths, and (b) resolve them only in the
build step that invokes the compiler.

This tagging scheme makes it possible to find ALL such targets in the
output of `image_feature` by simply traversing the JSON structure.  This
seems more flexible and less messy than maintaining a look-aside list of
targets whose paths the `image_layer` converter would need to resolve.
"""

load(":image_source.bzl", "image_source")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")
load(":wrap_runtime_deps.bzl", "maybe_wrap_executable_target")

_TargetTaggerInfo = provider(fields = ["targets"])

def new_target_tagger():
    return _TargetTaggerInfo(targets = {})

def tag_target(target_tagger, target, is_layer = False):
    target = normalize_target(target)
    target_tagger.targets[target] = 1  # Use a dict, since a target may recur
    return {("__BUCK_LAYER_TARGET" if is_layer else "__BUCK_TARGET"): target}

def extract_tagged_target(tagged):
    return tagged.get("__BUCK_TARGET") or tagged["__BUCK_LAYER_TARGET"]

def tag_required_target_key(tagger, d, target_key, is_layer = False):
    if target_key not in d:
        fail(
            "{} must contain the key {}".format(d, target_key),
        )
    d[target_key] = tag_target(tagger, target = d[target_key], is_layer = is_layer)

def tag_and_maybe_wrap_executable_target(target_tagger, target, wrap_prefix, **kwargs):
    was_wrapped, wrapped_target = maybe_wrap_executable_target(
        target,
        wrap_prefix,
        **kwargs
    )
    return was_wrapped, tag_target(target_tagger, wrapped_target)

def image_source_as_target_tagged_dict(target_tagger, user_source):
    src = structs.to_dict(image_source(user_source))
    if src.get("generator"):
        _was_wrapped, src["generator"] = tag_and_maybe_wrap_executable_target(
            target_tagger = target_tagger,
            target = src.pop("generator"),
            wrap_prefix = "image_source_wrap_generator",
            visibility = [],  # Not visible outside of project
        )
    else:
        is_layer = src["layer"] != None
        tag_required_target_key(
            target_tagger,
            src,
            "layer" if is_layer else "source",
            is_layer = is_layer,
        )
    return src

def target_tagger_to_feature(target_tagger, items, extra_deps = None):
    return struct(
        items = items,
        # We need to tell Buck that we depend on these targets, so
        # that `image_layer` can use `deps()` to discover its
        # transitive dependencies.
        #
        # This is a little hacky, because we are forcing these
        # targets to be built or fetched from cache even though we
        # don't actually use them until a later build step --- which
        # might be on a different host.
        #
        # Future: Talk with the Buck team to see if we can eliminate
        # this inefficiency.
        deps = list(target_tagger.targets.keys()) + (extra_deps or []),
    )
