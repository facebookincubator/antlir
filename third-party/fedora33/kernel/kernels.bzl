kernels = {
    "5.8.15-301.fc33.x86_64": struct(
        uname = "5.8.15-301.fc33.x86_64",
        artifacts = struct(
            devel = "//third-party/fedora33/kernel:5.8.15-301.fc33.x86_64-devel",
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
}
