load("//antlir/bzl:build_defs.bzl", "buck_genrule", "http_file")
load("//antlir/vm/bzl:build_kernel_artifacts.bzl", "build_kernel_artifacts")

def fedora_kernel(
        arch,
        kernel,
        fedora_release,
        core_sha256,
        headers_sha256,
        devel_sha256,
        headers_version = None):
    if not headers_version:
        headers_version = kernel
    url_f = "https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/{}/Everything/{}/os/Packages/k/kernel-{}-{}.rpm"
    http_file(
        name = kernel + "-core.rpm",
        sha256 = core_sha256,
        urls = [
            url_f.format(arch, fedora_release, "core", kernel),
        ],
    )
    http_file(
        name = kernel + "-headers.rpm",
        sha256 = headers_sha256,
        urls = [
            url_f.format(arch, fedora_release, "headers", headers_version),
        ],
    )
    http_file(
        name = kernel + "-devel.rpm",
        sha256 = devel_sha256,
        urls = [
            url_f.format(arch, fedora_release, "devel", kernel),
        ],
    )

    buck_genrule(
        name = kernel + "-rpm-exploded",
        out = ".",
        cmd = """
            cd $OUT
            rpm2cpio $(location :{kernel}-core.rpm) | cpio -idm
            # Removing build and source since they are symlinks that do not exist on the host
            rm -rf lib/modules/{kernel}/build lib/modules/{kernel}/source
        """.format(kernel = kernel),
    )

    buck_genrule(
        name = kernel + "-vmlinuz",
        out = "vmlinuz-" + kernel,
        cmd = "cp --reflink=auto $(location :{kernel}-rpm-exploded)/lib/modules/{kernel}/vmlinuz $OUT".format(kernel = kernel),
    )

    # Build kernel artifacts and get back a shape instance
    return build_kernel_artifacts(
        devel_rpm = ":{}-devel.rpm".format(kernel),
        headers_rpm = ":{}-headers.rpm".format(kernel),
        include_vmlinux = False,
        rpm_exploded = ":{}-rpm-exploded".format(kernel),
        uname = kernel,
    )
