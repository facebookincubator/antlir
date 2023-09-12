versions = {
    "aarch64": {
        "5.8.15-301.fc33.aarch64": struct(
            uname = "5.8.15-301.fc33.aarch64",
            artifacts = struct(
                devel = "//third-party/fedora33/kernel:5.8.15-301.fc33.aarch64-devel",
                headers = "//third-party/fedora33/kernel:5.8.15-301.fc33.aarch64-headers",
                modules = "//third-party/fedora33/kernel:5.8.15-301.fc33.aarch64-modules",
                vmlinuz = "//third-party/fedora33/kernel:5.8.15-301.fc33.aarch64-vmlinuz",
            ),
            version = struct(
                major = (5, 8),
                patch = 15,
                variant = 301,
                rc = None,
                flavor = "",
            ),
        ),
    },
    "x86_64": {
        "5.8.15-301.fc33.x86_64": struct(
            uname = "5.8.15-301.fc33.x86_64",
            artifacts = struct(
                devel = "//third-party/fedora33/kernel:5.8.15-301.fc33.x86_64-devel",
                headers = "//third-party/fedora33/kernel:5.8.15-301.fc33.x86_64-headers",
                modules = "//third-party/fedora33/kernel:5.8.15-301.fc33.x86_64-modules",
                vmlinuz = "//third-party/fedora33/kernel:5.8.15-301.fc33.x86_64-vmlinuz",
            ),
            version = struct(
                major = (5, 8),
                patch = 15,
                variant = 301,
                rc = None,
                flavor = "",
            ),
        ),
    },
}

def _get(uname, arch = "x86_64"):
    return versions[arch][uname]

def _selection():
    fail("not supported in oss")

kernels = struct(
    get = _get,
    select = struct(
        selection = _selection,
    ),
    all_kernels = ["5.8.15-301.fc33.x86_64"],
    default = _get("5.8.15-301.fc33.x86_64"),
    default_aarch64 = _get("5.8.15-301.fc33.aarch64", "aarch64"),
)
