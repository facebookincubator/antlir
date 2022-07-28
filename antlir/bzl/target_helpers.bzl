# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load(":oss_shim.bzl", "buck_genrule", "config", "repository_name", "target_utils")
load(":sha256.bzl", "sha256_b64")

def clean_target_name(name):
    # Used to remove invalid characters from target names.

    # chars that can be included in target name.
    valid_target_chars_str = ("ABCDEFGHIJKLMNOPQRSTUVWXYZ" +
                              "abcdefghijklmnopqrstuvwxyz" +
                              "0123456789" +
                              "_,.=-\\~@!+$")
    valid_target_chars = sets.make(
        [valid_target_chars_str[i] for i in range(len(valid_target_chars_str))],
    )

    # chars that can't be included in target name and should also be entirely
    # removed from name (replaced with ""). All characters not in `remove_chars`
    # and not in `valid_target_chars` are replaced with underscores to improve
    # readability.
    remove_chars_str = "][}{)(\"' "
    remove_chars = sets.make(
        [remove_chars_str[i] for i in range(len(remove_chars_str))],
    )

    return "".join(
        [
            name[i] if sets.contains(valid_target_chars, name[i]) else "_"
            for i in range(len(name))
            if not sets.contains(remove_chars, name[i])
        ],
    )

def normalize_target(target):
    if target.startswith("//"):
        return repository_name()[1:] + target

    # Don't normalize if already normalized. This avoids the Buck error:
    #   Error in package_name: Top-level invocations of package_name are not
    #   allowed in .bzl files.  Wrap it in a macro and call it from a BUCK file.
    if "//" in target:
        return target

    parsed = target_utils.parse_target(
        target,
        # The repository name always starts with "@", which we do not want here.
        # default_repo will be empty for the main repository, which matches the
        # results from $(query_targets ...).
        # @lint-ignore BUCKLINT
        default_repo = repository_name()[1:],
        # @lint-ignore BUCKLINT
        default_base_path = native.package_name(),
    )
    return target_utils.to_label(
        repo = parsed.repo,
        path = parsed.base_path,
        name = parsed.name,
    )

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`.
#
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
        # A 120-bit secure hash requires close to 2^60 targets to exist in one
        # project to trigger a birthday collision.  We don't need all 43 bytes.
    ) + "__" + sha256_b64(target)[:20]

# KEEP IN SYNC with its partial copy in `compiler/tests/sample_items.py`
def wrap_target(target, wrap_suffix):
    target = normalize_target(target)
    _, name = target.split(":")
    wrapped_target = name + "__" + wrap_suffix + "-" + mangle_target(target)
    wrapped_target = clean_target_name(wrapped_target)
    return native.rule_exists(wrapped_target), wrapped_target

def targets_and_outputs_arg_list(name, query):
    """
    NOTE: This is important.

    This will return a list that contains a fully constructed CLI parameter and
    it's argument suitable for use in `antlir` CLIs that require a mapping of
    dependency targets -> on disk locations. This mapping between target names
    and on disk locations is inherently uncacheable. As such, it must be
    provided at the last possible step. The only reasonable place this can be
    done is when the arguments passed to a buck runnable CLI are generated or
    when the target that is being `run` is actually built. ie:, when
    `buck run //path/to/some/layer=container` is invoked, only when the
    `//path/to/some/layer=container` target is *actually* built should this
    mapping be constructed. Any other time is a recipe for potential caching
    problems that are hard to debug.

    Use this `.bzl` macro to generate the CLI arg in conjunction with the
    `antlir.cli.add_targets_and_outputs_arg` helper to consume the arg.

    The actual mapping that is consumed by the CLI arg is generated as a json
    file with the help of the `//antlir:serialize-targets-and-outputs` binary.
    Using a json file works around the limitations that buck has dealing
    with proper shell quoting.
    """

    if not query:
        fail("`targets_and_outputs_arg_list` requires a query built with `//antlir/bzl:query.bzl`")

    target = "{}__deps-targets-to-outputs-{}".format(name, sha256_b64(name + query))
    if not native.rule_exists(target):
        buck_genrule(
            name = target,
            out = ".",
            bash = """
echo -n "$(query_targets_and_outputs {delim} '{query}')" | \
$(exe {serialize_targets_and_outputs}) "{delim}" > "$OUT/targets-and-outputs.json"
echo -n '{query}' > "$OUT/query"
            """.format(
                delim = "<|ThisDelimiterIsSizzlin|>",
                serialize_targets_and_outputs = antlir_dep(":serialize-targets-and-outputs"),
                query = query,
            ),
            antlir_rule = "user-internal",
            # This cannot be cacheable because it is generating machine
            # specific paths.
            cacheable = False,
        )

    return ["--targets-and-outputs", "$(location :{})/targets-and-outputs.json".format(target)]

def antlir_dep(target):
    """Get a normalized target referring to a dependency under the root Antlir
    directory. This helper should be used when referring to any Antlir target,
    excluding `load` statements. This should not be used when referring to
    targets defined outside of the Antlir directory, e.g. user-defined layers in
    external build files.

    For example, if you want to refer to <cell>//antlir:compiler, the dependency
    should be expressed as `antlir_dep(":compiler")`. Similarly, if you want to
    refer to <cell>//antlir/nspawn_in_subvol:run, the dependency should be
    expressed as `antlir_dep("nspawn_in_subvol:run")`.

    Specifically, this will add the Antlir cell name, and the "antlir" prefix
    to the package path, which will ensure the target is resolved correctly and
    will help when moving Antlir to its own cell."""

    if "//" in target or target.startswith("/"):
        fail(
            "Antlir deps should be expressed as a target relative to the " +
            "root Antlir directory, e.g. instead of `<cell>//antlir/foo:bar` " +
            "the dep should be expressed as `foo:bar`.",
        )

    if target.startswith(":"):
        return "{}//antlir{}".format(config.get_antlir_cell_name(), target)
    return "{}//antlir/{}".format(config.get_antlir_cell_name(), target)
