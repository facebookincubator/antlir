kernels = {
    "5.3.7-301.fc31.x86_64": struct(
        uname = "5.3.7-301.fc31.x86_64",
        devel = "//third-party/fedora31/kernel:5.3.7-301.fc31.x86_64-devel.rpm",
        modules = "//third-party/fedora31/kernel:5.3.7-301.fc31.x86_64-modules",
        headers = "//third-party/fedora31/kernel:5.3.6-300.fc31.x86_64-headers.rpm",
        vmlinuz = "//third-party/fedora31/kernel:5.3.7-301.fc31.x86_64-vmlinuz",
        version = struct(
            major = (5, 3),
            patch = 7,
            variant = 301,
            rc = None,
            flavor = "",
        )
    ),
}
