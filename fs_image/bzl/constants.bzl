DO_NOT_USE_BUILD_APPLIANCE = "__DO_NOT_USE_BUILD_APPLIANCE__"

DEFAULT_BUILD_APPLIANCE = native.read_config(
    "fs_image",
    "default_build_appliance",
)

DEFAULT_RPM_INSTALLER = native.read_config("fs_image", "default_rpm_installer")
