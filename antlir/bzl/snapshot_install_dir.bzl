load("@bazel_skylib//lib:paths.bzl", "paths")
load(":target_helpers.bzl", "mangle_target")

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
RPM_SNAPSHOT_BASE_DIR = "__antlir__/rpm/repo-snapshot"

# Here are the ways to specify a snapshot:
#
#  - An /__antlir__ path that's valid in the container.  This is mainly used
#    like so: `/__antlir__/rpm/default-snapshot-for-installer/...`.
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
        return paths.join("/", RPM_SNAPSHOT_BASE_DIR, mangle_target(snapshot))
    if snapshot.startswith("/__antlir__/rpm/"):
        return snapshot
    fail("Bad RPM snapshot; see `snapshot_install_dir`: {}".format(snapshot))
