# This reads a set of configs from the `.buckconfig` and populates a simple
# struct.  That is then turned into json and loaded by the python side of
# the `nspawn_in_subvol` sub system.  In the future this would be implemented
# via a `Shape` so that the typing can be maintained across bzl/python.
# Also note that `buck` doesn't properly support lists in configs, hence
# the need for the `split(",")`.  If and when buck supports lists properly
# this should be fixed.
def get_repo_config():
    return struct(
        repo_artifacts_host_mounts = native.read_config("fs_image", "repo_artifacts_host_mounts", "").split(","),
    )
