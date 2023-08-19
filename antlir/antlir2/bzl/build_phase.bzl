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
    # Clean up litter left behind when running chef-solo
    "chef_cleanup",
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
    # Stamp build info into the built layer
    "buildinfo_stamp",
)

# Quick self-test to ensure that order is correct
if list(BuildPhase.values()) != [
    "package_manager",
    "chef_setup",
    "chef",
    "chef_cleanup",
    "genrule",
    "remove",
    "compile",
    "buildinfo_stamp",
]:
    fail("BuildPhase.values() is no longer in order. This will produce incorrect image builds.")
