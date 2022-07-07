# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":oss_shim.bzl", "buck_genrule")
load(":target_helpers.bzl", "wrap_target")

def logging_command_wrapper(
        target,
        wrap_suffix,
        logging_command):
    """
    IMPORTANT: Never reuse a `wrap_suffix` with two different logging
    commands.  If you do, one of your logging commands may silently fail to
    run.  The best prevention is to use a DESCRIPTIVE and UNIQUE
    `wrap_suffix` that no other use-case would type by accident.

    Defines a wrapper target named `<target>__<wrap_suffix>...` and returns
    its path (i.e.  with `:` prefixed).  The wrapper outputs the same exact
    file as `target`, in a zero-copy fashion.

    `logging_command` is a bash snippet that can use `"$TARGET_PATH"` to
    reference the output location of `target`.

    Depend on this wrapper instead of `target` to ensure that your logging
    code gets invoked whenever `target` is rebuilt.
    """

    # Caveat: It would seem attractive to also hash `logging_command`, to
    # prevent collisions when `wrap_suffix` isn't unique enough.  However,
    # in practice, we want to be able to vary the logging command for
    # different Buck config settings, but the target graph (and thus target
    # naming) must rename the same.
    wrapper_exists, wrapper_name = wrap_target(target, wrap_suffix)
    if wrapper_exists:
        # Tolerate duplicate calls to `logging_command_wrapper` in one project.
        return ":" + wrapper_name

    buck_genrule(
        name = wrapper_name,
        out = "an-exact-copy-of-the-input",
        bash = '''\
set -ue -o pipefail
TARGET_PATH=$(location {target})
{logging_command}
# Preserve FS metadata to make this target truly transparent.  
#
# On btrfs or XFS, copy-on-write matches what Buck expects perfectly.
# Otherwise, hardlinks should be OK (since build artifacts in `buck-out`
# should never be mutated anyway), but it carries some risk of fragility,
# since it's not transparent from the point of view of the filesystem.
# Symlinks are even worse, not least because they'd need to encode the
# absolute path to the repo, and thus be broken if the repo moves.
#
# We need `-f` the fallback `ln` because `cp` leaves behind a 0-sized file
# when `--reflink=always` fails: https://fburl.com/oj17j4su
cp --preserve=all --reflink=always "$TARGET_PATH" "$OUT" ||
    ln -f "$TARGET_PATH" "$OUT"
'''.format(
            target = target,
            logging_command = logging_command,
        ),
        # RE workers don't have network access, but logging code will
        # typically write to Scuba or similar.  Moreover, the only cost of
        # running this locally is that the resulting input file has to be
        # downloaded from RE to the local machine, which is possibly
        # something that you have to do anyway.
        labels = ["network_access"],
        # Don't pollute the distributed Buck caches with a copy of the input.
        cacheable = False,
        # Do not count this log wrapper for the purposes of "CI dep distance".
        antlir_rule = "user-internal",
    )
    return ":" + wrapper_name
