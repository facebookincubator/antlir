DO_NOT_USE_BUILD_APPLIANCE = "__DO_NOT_USE_BUILD_APPLIANCE__"

DEFAULT_BUILD_APPLIANCE = native.read_config(
    "fs_image",
    "default_build_appliance",
)

DEFAULT_RPM_INSTALLER = native.read_config("fs_image", "default_rpm_installer")

# This needs to be kept in sync with
# `fs_image.nspawn_in_subvol.args._QUERY_TARGETS_AND_OUTPUTS_SEP`
QUERY_TARGETS_AND_OUTPUTS_SEP = "|"
