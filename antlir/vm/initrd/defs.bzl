load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "kernel_get", "third_party")

def create_initrd_targets():
    """
    Create an initrd target for each known kernel version.
    """
    for uname, kernel in kernel_get.versions.items():
        initrd("{}-initrd".format(uname), kernel)

VM_MODULE_LIST = [
    "drivers/block/virtio_blk.ko",
    "fs/9p/9p.ko",
    "net/9p/9pnet.ko",
    "net/9p/9pnet_virtio.ko",
]

def initrd(name, kernel):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine and setup the root disk as a btrfs seed device
    with the second disk for writes to go to.

    The init is built "from scratch" with busybox which allows us easier
    customization as well as much faster build time than using dracut.
    """

    busybox = [
        image.install(
            dest = "/bin/busybox",
            source = third_party.library("busybox", "bin/busybox"),
        ),
    ] + [
        image.symlink_file(
            "/bin/busybox",
            "/bin/" + applet,
        )
        for applet in [
            "cat",
            "chroot",
            "cp",
            "depmod",
            "dmesg",
            "file",
            "insmod",
            "ln",
            "ls",
            "lsmod",
            "mdev",
            "mkdir",
            "modprobe",
            "mount",
            "sh",
            "switch_root",
            "umount",
            "uname",
        ]
    ]

    # This intermediate genrule is here to create a dir hierarchy
    # of kernel modules that are needed for the initrd.  This
    # provides a single dir that can be cloned into the initrd
    # layer and allows for kernel modules that might be missing
    # from different kernel builds.
    buck_genrule(
        name = name + "--modules",
        out = ".",
        cmd = """
            set -x
            mkdir -p $OUT
            pushd $OUT 2>/dev/null
            mod_source="$(location {kernel_modules})"
            mods="{module_list}"
            for mod in $mods; do
                if [[ -f "$mod_source/$mod" ]]; then
                    mod_dir=\\$(dirname "$mod")
                    mkdir -p "$mod_dir"
                    cp "$mod_source/$mod" "$mod_dir"
                fi
            done
        """.format(
            kernel_modules = kernel.modules,
            module_list = " ".join(VM_MODULE_LIST),
        ),
        antlir_rule = "user-internal",
        visibility = [],
    )

    module_base_dir = "/lib/modules/" + kernel.uname
    image.layer(
        name = name + "--layer",
        features = [
            # Setup the init script
            image.install(
                dest = "/init",
                source = ":init.sh",
                mode = "u+rwx,og+rx",
            ),
            image.mkdir("/", "bin"),
            image.mkdir("/", "newroot"),
            image.mkdir("/", "proc"),
            image.mkdir("/", "sys"),
            image.mkdir("/", "tmp"),
            image.mkdir("/", module_base_dir),
            busybox,
            image.install(
                image.source(
                    source = ":" + name + "--modules",
                    path = ".",
                ),
                module_base_dir + "/kernel",
            ),
        ],
        visibility = ["//antlir/..."],
    )

    image.package(
        name = name,
        layer = ":" + name + "--layer",
        format = "cpio.gz",
        visibility = ["//antlir/..."],
    )
