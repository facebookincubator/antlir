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

load(":crc32.bzl", "hex_crc32")
load(":oss_shim.bzl", "target_utils")
load(":image_source.bzl", "image_source")
load(":wrap_runtime_deps.bzl", "maybe_wrap_runtime_deps_as_build_time_deps")

_TargetTaggerInfo = provider(fields = ["targets"])

def new_target_tagger():
    return _TargetTaggerInfo(targets = {})

def normalize_target(target):
    parsed = target_utils.parse_target(
        target,
        # $(query_targets ...) omits the current repo/cell name
        default_repo = "",
        default_base_path = native.package_name(),
    )
    return target_utils.to_label(
        repo = parsed.repo,
        path = parsed.base_path,
        name = parsed.name,
    )

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

# Makes a deterministic and unique "nonce" from a target path, which can
# itself be used as part of a target name.  Its form is:
#   <original target name prefix>...<original target name suffix>__<hash>
#
# DO NOT RELY ON THE DETAILS OF THIS MANGLING -- they are subject to change.
#
# `min_abbrev` guarantees that the suffix & prefix will never be shorter
# than that many characters.  Including the original target is intended to
# aid debugging.  At the same time, we don't want to mangle the full target
# path since that can easily exceed the OS's maximum filename length.
#
# The hash is meant to disambiguate identically-named targets from different
# directories.
def mangle_target(target, min_abbrev = 15):
    # The target to wrap may be in a different directory, so we normalize
    # its path to ensure the hashing is deterministic.  This means that
    # `wrap_target` below can reuse identical "wrapped" targets that are
    # requested from the same project (aka BUCK/TARGETS file).
    target = normalize_target(target)

    _, name = target.split(":")
    return (
        name if len(name) < (2 * min_abbrev + 3) else (
            name[:min_abbrev] + "..." + name[-min_abbrev:]
        )
    ) + "__" + hex_crc32(target)

def wrap_target(target, wrap_prefix):
    # The wrapper target is plumbing, so it will start with the provided
    # prefix to hide it from e.g. tab-completion.
    wrapped_target = wrap_prefix + "__" + mangle_target(target)
    return native.rule_exists(wrapped_target), wrapped_target

def tag_and_maybe_wrap_executable_target(target_tagger, target, wrap_prefix, **kwargs):
    exists, wrapped_target = wrap_target(target, wrap_prefix)

    if exists:
        return True, tag_target(target_tagger, ":" + wrapped_target)

    # The `wrap_runtime_deps_as_build_time_deps` docblock explains this:
    was_wrapped, maybe_target = maybe_wrap_runtime_deps_as_build_time_deps(
        name = wrapped_target,
        target = target,
        **kwargs
    )
    return was_wrapped, tag_target(target_tagger, maybe_target)

def image_source_as_target_tagged_dict(target_tagger, user_source):
    src = image_source(user_source)._asdict()
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
