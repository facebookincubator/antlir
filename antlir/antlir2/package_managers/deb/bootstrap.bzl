# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Bootstrap an OSS apt build appliance without requiring an existing one."""

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo")
load("//antlir/bzl:internal_external.bzl", "is_facebook")

# debootstrap --variant=minbase --include=apt,python3,python3-apt,mount
# --print-debs trixie: the base system plus apt's Python bindings. Keep this
# list in sync when changing the bootstrap distribution.
BOOTSTRAP_PACKAGES = [
    "apt",
    "base-files",
    "base-passwd",
    "bash",
    "bsdutils",
    "coreutils",
    "dash",
    "debconf",
    "debian-archive-keyring",
    "debianutils",
    "diffutils",
    "distro-info-data",
    "dpkg",
    "findutils",
    "gcc-14-base",
    "grep",
    "gzip",
    "hostname",
    "init-system-helpers",
    "libacl1",
    "libapt-pkg7.0",
    "libattr1",
    "libaudit-common",
    "libaudit1",
    "libblkid1",
    "libbsd0",
    "libbz2-1.0",
    "libc-bin",
    "libc6",
    "libcap-ng0",
    "libcap2",
    "libcrypt1",
    "libdb5.3t64",
    "libdebconfclient0",
    "libexpat1",
    "libffi8",
    "libgcc-s1",
    "libgmp10",
    "libhogweed6t64",
    "liblastlog2-2",
    "liblz4-1",
    "liblzma5",
    "libmd0",
    "libmount1",
    "libncursesw6",
    "libnettle8t64",
    "libpam-modules",
    "libpam-modules-bin",
    "libpam-runtime",
    "libpam0g",
    "libpcre2-8-0",
    "libpython3-stdlib",
    "libpython3.13-minimal",
    "libpython3.13-stdlib",
    "libreadline8t64",
    "libseccomp2",
    "libselinux1",
    "libsemanage-common",
    "libsemanage2",
    "libsepol2",
    "libsmartcols1",
    "libsqlite3-0",
    "libssl3t64",
    "libstdc++6",
    "libsystemd0",
    "libtinfo6",
    "libudev1",
    "libuuid1",
    "libxxhash0",
    "libzstd1",
    "login",
    "login.defs",
    "mawk",
    "media-types",
    "mount",
    "ncurses-base",
    "ncurses-bin",
    "netbase",
    "openssl-provider-legacy",
    "passwd",
    "perl-base",
    "python-apt-common",
    "python3",
    "python3-apt",
    "python3-minimal",
    "python3.13",
    "python3.13-minimal",
    "readline-common",
    "sed",
    "sqv",
    "sysvinit-utils",
    "tar",
    "tzdata",
    "util-linux",
    "zlib1g",
]

def _impl(ctx: AnalysisContext) -> list[Provider]:
    root = ctx.actions.declare_output("root", dir = True)
    ctx.actions.run(
        cmd_args("bash", ctx.attrs._script, root.as_output(), ctx.attrs.debs),
        category = "deb_bootstrap",
    )
    return [DefaultInfo(root), BuildApplianceInfo(dir = root)]

_bootstrap = rule(
    impl = _impl,
    attrs = {
        "debs": attrs.list(attrs.source()),
        "_script": attrs.default_only(attrs.source(default = "antlir//antlir/antlir2/package_managers/deb:bootstrap.sh")),
    },
)

_bootstrap_macro = rule_with_default_target_platform(_bootstrap)

def bootstrap(**kwargs):
    if is_facebook:
        return
    native.export_file(name = "bootstrap.sh")
    _bootstrap_macro(**kwargs)
