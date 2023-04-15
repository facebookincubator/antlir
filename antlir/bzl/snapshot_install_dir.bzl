# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load(":target_helpers.bzl", "mangle_target")

ANTLIR_DIR = "/__antlir__"

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
RPM_SNAPSHOT_BASE_DIR = "/__antlir__/rpm/repo-snapshot"

# KEEP IN SYNC with the copy in `fs_utils.py`
RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR = "/__antlir__/rpm/default-snapshot-for-installer"

# Here are the ways to specify a snapshot:
#
#  - An /__antlir__ path that's valid in the container.  This is mainly used
#    like so: `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR + "/prog"`.
#
#  - A Buck target path.  But, it is **not** used to depend on a Buck
#    target.  A target may not even exist in the current repo at this path.
#    Rather, this target path is normalized, mangled, and then used to
#    select a non-default child of `/__antlir__/rpm/repo-snapshot/` in the
#    build appliance.  So this refers to a target that got incorporated into
#    the build appliance, whenever that image was built.
#
# KEEP THE `mangle_target` PART IN SYNC with its copy in `rpm/find_snapshot.py`
def snapshot_install_dir(snapshot):
    if ":" in snapshot:
        # remove various suffixes from the snapshot target
        path, tgt = snapshot.split(":")
        if ".rc" in tgt:
            tgt = tgt.split(".rc")[0]
        if tgt.endswith(".layer"):
            tgt = tgt[:-len(".layer")]
        snapshot = path + ":" + tgt
        return paths.join(RPM_SNAPSHOT_BASE_DIR, mangle_target(snapshot))
    if snapshot.startswith("/__antlir__/rpm/"):
        return snapshot
    fail("Bad RPM snapshot; see `snapshot_install_dir`: {}".format(snapshot))
