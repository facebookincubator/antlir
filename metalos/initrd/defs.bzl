load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/genrule/extractor:extract.bzl", "extract")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/feature:usergroup.bzl", "SHELL_NOLOGIN")
load("//antlir/bzl/linux:defs.bzl", "linux")
load(":systemd.bzl", "clone_systemd_configs", SYSTEMD_BINARIES = "BINARIES")

def dbus():
    return [
        feature.group_add("dbus"),
        feature.user_add("dbus", "dbus", "/", SHELL_NOLOGIN),
    ]

def debug():
    return [
        systemd.install_dropin("//metalos/initrd:debug-shell.conf", "debug-shell.service"),
        systemd.install_dropin("//metalos/initrd:emergency.conf", "emergency.service"),
    ]

def users():
    return [
        feature.install("//metalos/initrd:group", "/etc/group"),
        feature.install("//metalos/initrd:passwd", "/etc/passwd"),
        feature.install("//antlir:empty", "/etc/shadow"),
        feature.install("//antlir:empty", "/usr/sbin/nologin"),
        feature.group_add("systemd-network"),
        feature.user_add(
            "systemd-network",
            "systemd-network",
            "/",
            SHELL_NOLOGIN,
        ),
    ]

def udev(source):
    return [
        feature.ensure_subdirs_exist("/usr/lib/", "udev/rules.d"),
    ] + [
        feature.install(
            "//metalos/initrd/udev:{}".format(f),
            "/usr/lib/udev/rules.d/{}".format(f),
        )
        for f in [
            "10-dm.rules",
            "50-udev-default.rules",
            "60-block.rules",
            "60-persistent-storage.rules",
            "75-net-description.rules",
            "80-drivers.rules",
            "80-net-setup-link.rules",
            "95-dm-notify.rules",
            "99-systemd.rules",
        ]
    ] + [
        # Some of helper executables that udev calls for parsing e.g. serial numbers.
        feature.clone(
            source,
            "/usr/lib/udev/{}".format(f),
            "/usr/lib/udev/{}".format(f),
        )
        for f in [
            "ata_id",
            "dmi_memory_id",
            "scsi_id",
        ]
    ]

def build_initrd_base(
        name,
        source,
        os_name = "MetalOS",
        **kwargs):
    # A base initrd with only systemd + essentials included
    image.layer(
        name = name,
        features = [
            linux.filesystem.install(),
            clone_systemd_configs(source),
            feature.ensure_file_symlink("/usr/lib/systemd/systemd", "/init"),
            # Systemd uses the presence of /etc/initrd-release to determine if it
            # is running in an initrd, however there are cases where it only parses
            # /{etc,usr/lib}/os-release, so just cover all our bases with symlinks
            linux.release.install(
                path = "/usr/lib/initrd-release",
                layer = ":" + name,
                os_name = os_name,
                variant = "Initrd",
            ),
            feature.ensure_file_symlink("/usr/lib/initrd-release", "/etc/initrd-release"),
            feature.ensure_file_symlink("/usr/lib/initrd-release", "/etc/os-release"),
            feature.ensure_file_symlink("/usr/lib/initrd-release", "/usr/lib/os-release"),
            # With our custom setup, just enable initrd-cleanup.service directly in
            # initrd.target instead of going through initrd-parse-etc.service as normal
            systemd.enable_unit(
                "initrd-cleanup.service",
                target = "initrd.target",
            ),
            systemd.install_dropin(
                "dropins/reload-before-cleanup.conf",
                unit = "initrd-cleanup.service",
            ),
            debug(),
            dbus(),
            linux.config.glibc.nsswitch.install(linux.config.glibc.nsswitch.default),
            users(),
            udev(source),
            extract.extract(
                binaries = SYSTEMD_BINARIES + [
                    # Metalctl uses libblkid for device discover, so include
                    # blkid.8 for debugging purposes.
                    "/usr/sbin/blkid",
                    "/usr/sbin/btrfs",
                    # This is pretty useful for debugging hardware discovery
                    # issues in the initrd.  It adds ~970k of weight, which
                    # isn't light, but considered worth the tradeoff for the
                    # benefits.
                    "/usr/sbin/lshw",
                ],
                dest = "/",
                source = source,
            ),
        ],
        **kwargs
    )
