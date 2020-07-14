"""
DO NOT DEPEND ON THIS TARGET DIRECTLY, except through the `features=` field of
`image.feature` or `image.layer`.  A direct dependency will not work the way
you expect, and you will end up with incorrect behavior.

## Composing images using `image.feature`

When building regular binaries, one will often link multiple independent
libraries that know nothing about one another. Each of those libraries may
depend on other libraries, and so forth.

This ability to **compose** largely uncoupled pieces of functionality is an
essential tool of a software engineer.

`image.feature` is a way of bringing the same sort of compositionality to
building filesystem images.

A feature specifies a set of **items**, each of which describes some aspect
**of a desired end state** for the filesystem.  Examples:
 - A directory must exist.
 - A tarball must be extracted at this location.
 - An RPM must be installed, or must be **ABSENT** from the filesystem.
 - Other `image.feature` that must be installed.

Importantly, the specifications of an `image.feature` are not ordered. They are
not commands or instructions.  Rather, they are a declaration of what should be
true. You can think of a feature as a thunk or callback.

In order to convert the declaration into action, one makes an `image.layer`.
Read that target's docblock for more info, but in essence, that will:
 - specify the initial state of the filesystem (aka the parent layer)
 - verify that the features can be applied correctly -- that dependencies are
   satisfied, that no features provide duplicate paths, etc.
 - install the features in dependency order,
 - capture the resulting filesystem, ready to be used as another parent layer.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule", "get_visibility")
load("//fs_image/bzl:target_helpers.bzl", "normalize_target")
load("//fs_image/bzl:target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")

# ## Why are `image.feature`s forbidden as dependencies?
#
# The long target suffix below exists to discourage people from directly
# depending on `image.feature`s.  They are not real targets, but rather a
# language feature to make it easy to compose independent features of container
# images.
#
# A normal Buck target is supposed to produce an output that completely
# encapsulates the outputs of all of its dependencies (think static linking),
# so in deciding whether to build a file or use a cached output, Buck will only
# consider direct dependencies, not transitive ones.
#
# In contrast, `image.feature` simply serializes its keyword arguments to JSON.
# It does not consume the outputs of its dependencies -- it reads neither
# regular target outputs, nor the JSONs of the `image_feature`s, on which it
# depends.
#
# By violating Buck semantics, `image_features` creates two problems for
# targets that might depend on them:
#
# 1) Buck will build any target depending on an `image_feature` immediately
#    upon ensuring that its JSON output exists in the output tree.  It is
#    possible that the output tree lacks, or contains stale versions of, the
#    outputs of the targets, on which the `image_feature` itself depends.
#
# 2) If the output of a dependency of an `image.feature` changes, this will
#    cause the feature to rebuild.  However, the output of the `image.feature`
#    will remain unchanged, and so any target depending on the `image.feature`
#    will **NOT** get rebuilt.
#
# For these reasons, special logic is required to correctly depend on
# `image.feature` targets.  At the moment, we are not aware of any reason to
# have direct access to the `image.feature` JSON outputs in any case.  Most
# users will want to depend on build artifacts that are downstream of
# `image.feature`, like `image.layer`.
#
# Maintainers of this code: please change this string at will, **without**
# searching the codebase for people who might be referring to it.  They have
# seen this blob, and they have agreed to have their code broken without
# warning.  Do not incentivize hacky engineering practices by "being nice."
# (Caveat: don't change it daily to avoid forcing excessive rebuilds.)
DO_NOT_DEPEND_ON_FEATURES_SUFFIX = (
    "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN_" +
    "SO_DO_NOT_DO_THIS_EVER_PLEASE_KTHXBAI"
)

# Use mutual recursion so that buildifier doesn't complain about recursion.
# Allows for easier handling of aribtary depth feature nesting.
def _assign_target_to_features_trololo(*args):
    return _assign_target_to_features(*args)

def _assign_target_to_features(features, target):
    for feature in features:
        feature["target"] = target
        _assign_target_to_features_trololo(feature.get("features", []), target)

def normalize_features(porcelain_targets_or_structs, human_readable_target):
    targets = []
    inline_features = []
    direct_deps = []
    for f in porcelain_targets_or_structs:
        if types.is_string(f):
            targets.append(f + DO_NOT_DEPEND_ON_FEATURES_SUFFIX)
        else:
            direct_deps.extend(f.deps)
            inline_features.append(f.items._asdict())
            _assign_target_to_features(inline_features, human_readable_target)

    return struct(
        targets = targets,
        inline_features = inline_features,
        direct_deps = direct_deps,
    )

def private_do_not_use_feature_json_genrule(name, deps, output_feature_cmd, visibility):
    buck_genrule(
        # The constant declaration explains the reason for the name change.
        name = name + DO_NOT_DEPEND_ON_FEATURES_SUFFIX,
        out = "feature.json",
        type = "image_feature",  # For queries
        # Future: It'd be nice to refactor `image_utils.bzl` and to use the
        # log-on-error wrapper here (for `fetched_package_layer`).
        bash = """
        # {deps}
        set -ue -o pipefail
        {output_feature_cmd}
        """.format(
            deps = " ".join([
                "$(location {})".format(t)
                for t in sorted(deps)
            ]),
            output_feature_cmd = output_feature_cmd,
        ),
        visibility = visibility,
        fs_image_internal_rule = True,
    )

def image_feature(name = None, features = None, visibility = None):
    """This is the main image.feature() interface.

    It doesn't define any actions itself (there are more specific rules for the
    actions), but image.feature() serves three purposes:

    1) To group multiple features, using the features = [...] argument.

    2) To give the features a name, so they can be referred to using a
       ":buck_target" notation.

    3) To specify a custom visibility for a set of features.

    For features that execute actions that are used to build the container
    (install RPMs, remove files/directories, create symlinks or directories,
    copy executable or data files, declare mounts), see the more specific
    features meant for a specific purpose.

    To understand the concept, read "Composing images using image_feature" in the file docblock.
    """

    # (1) Normalizes & annotates Buck target names so that they can be
    #     automatically enumerated from our JSON output.
    # (2) Builds a list of targets so that this converter can tell Buck
    #     that the `image_feature` depends on it.
    target_tagger = new_target_tagger()

    # Omit the ugly suffix here since this is meant only for humans to read while debugging.
    # For inline targets, `image_layer.bzl` sets this to the layer target path.
    human_readable_target = normalize_target(":" + name) if name else None
    normalized_features = normalize_features(
        features or [],
        human_readable_target,
    )

    feature = target_tagger_to_feature(
        target_tagger,
        items = struct(
            target = human_readable_target,
            features = [
                tag_target(target_tagger, t)
                for t in normalized_features.targets
            ] + normalized_features.inline_features,
        ),
        extra_deps = normalized_features.direct_deps + [
            # The `fake_macro_library` docblock explains this self-dependency
            "//fs_image/bzl/image_actions:feature",
        ],
    )

    # Anonymous features do not emit a target, but can be used inline as
    # part of an `image.layer`.
    if not name:
        return feature

        # Serialize the arguments and defer our computation until build-time.

    # This allows us to automatically infer what is provided by RPMs & TARs,
    # and makes the implementation easier.
    #
    # Caveat: if the serialization exceeds the kernel's MAX_ARG_STRLEN,
    # this will fail (128KB on the Linux system I checked).
    #
    # TODO: Print friendlier error messages on user error.
    private_do_not_use_feature_json_genrule(
        name = name,
        deps = feature.deps,
        output_feature_cmd = 'echo {out} > "$OUT"'.format(
            out = shell.quote(feature.items.to_json()),
        ),
        visibility = get_visibility(visibility, name),
    )

    # NB: it would be easy to return the path to the new feature target
    # here, enabling the use of named features inside `features` lists of
    # layers, but this seems like an unreadable pattern, so instead:
    return None
