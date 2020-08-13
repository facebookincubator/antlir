DO_NOT_USE_BUILD_APPLIANCE = "__DO_NOT_USE_BUILD_APPLIANCE__"

DEFAULT_BUILD_APPLIANCE = native.read_config(
    "fs_image",
    "default_build_appliance",
)

# KEEP THIS DICTIONARY SMALL.
#
# For each `image.feature`, we have to emit as many targets as there are
# elements in this list, because we do not know the version set that the
# including `image.layer` will use.  This would be fixable if Buck supported
# providers like Bazel does.
_STR_VERSION_SET_TO_PATH = native.read_config("fs_image", "version_set_to_path")
_LIST_VERSION_SET_TO_PATH = (
    _STR_VERSION_SET_TO_PATH.split(":") if _STR_VERSION_SET_TO_PATH else []
)
VERSION_SET_TO_PATH = dict(
    zip(_LIST_VERSION_SET_TO_PATH[::2], _LIST_VERSION_SET_TO_PATH[1::2]),
)
if 2 * len(VERSION_SET_TO_PATH) != len(_LIST_VERSION_SET_TO_PATH):
    fail("fs_image.version_set_to_path must be a dict: k1:v1:k2:v2")

# A layer can turn off version locking via `version_set = None`.
VERSION_SET_TO_PATH[None] = None

DEFAULT_VERSION_SET = native.read_config(
    "fs_image",
    "default_version_set",
) or None

DEFAULT_RPM_INSTALLER = native.read_config("fs_image", "default_rpm_installer")

# This needs to be kept in sync with
# `fs_image.nspawn_in_subvol.args._QUERY_TARGETS_AND_OUTPUTS_SEP`
QUERY_TARGETS_AND_OUTPUTS_SEP = "|"
