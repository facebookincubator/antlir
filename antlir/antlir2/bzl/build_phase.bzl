# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Different phases of hidden layers. These will be executed in the order in
# which they are defined in this list. Phases are independently cached, and any
# changes to features will only cause rebuilds of downstream phases.
BuildPhase = enum(
    # The feature installs/removes packages with an OS package manager that allows
    # unpredictable side effects (file creation/deletion, {user,group}
    # creation/deletion, etc)
    "package_manager",
    # chef_solo is the worst-behaved antlir2 feature. This build phase exists so
    # that we can install solo bundles into the image before running chef which
    # gives better visibility than bind-mounting them in at compile time
    "chef_setup",
    # Chef could really be part of "genrule", but breaking it out into its own
    # phase lets us more explicitly order it, while also clearly attributing
    # slowness to the user in the buck logs
    "chef",
    # We have no idea what this is going to do, but ordering it after
    # 'package_manager' will allow the user to install dependencies in the same
    # layer.
    "genrule",
    # Removing files/directories from the parent layer or
    # 'package_manager'/'genrule' features within the same layer is hard to
    # correctly topologically sort because it requires moving pre-existing
    # edges in the graph. If we just put it in its own phase, it's easily
    # treated as a parent layer and any features that try to interact with the
    # removed paths (eg replace one of them) will just work.
    "remove",
    # Generally well-behaved image features that are predictable and
    # topologically orderable. Everything should go here that doesn't have to be
    # in one of the earlier, less well-behaved phases.
    "compile",
)

# Quick self-test to ensure that order is correct
if list(BuildPhase.values()) != [
    "package_manager",
    "chef_setup",
    "chef",
    "genrule",
    "remove",
    "compile",
]:
    fail("BuildPhase.values() is no longer in order. This will produce incorrect image builds.")

def _is_predictable(phase: BuildPhase.type) -> bool.type:
    """
    If a BuildPhase is predictable, we don't have to crawl the filesystem tree
    to discover "dynamic" items, like we do to discover things like rpm-created
    users or file paths.

    Note that "predictable" is not the same as "deterministic". All phases are
    deterministic/repeatable, but some have effects on the built image that
    cannot be determined in advance without actually building them.
    """
    return {
        "chef": False,
        "chef_setup": True,  # using regular well-behaved features
        "compile": True,
        "genrule": False,
        "package_manager": False,
        "remove": False,  # for directories, ensure that all the children are removed
    }[phase.value]

build_phase = struct(
    is_predictable = _is_predictable,
)
