load(":crc32.bzl", "hex_crc32")
load(":oss_shim.bzl", "target_utils")

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
    ) + "__" + hex_crc32(target)

def wrap_target(target, wrap_prefix):
    # The wrapper target is plumbing, so it will start with the provided
    # prefix to hide it from e.g. tab-completion.
    wrapped_target = wrap_prefix + "__" + mangle_target(target)
    return native.rule_exists(wrapped_target), wrapped_target
