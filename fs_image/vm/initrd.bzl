load("//fs_image/bzl:oss_shim.bzl", "buck_genrule", "third_party")

def initrd(name, uname, modules = None):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine and setup the root disk as a btrfs seed device
    with the second disk for writes to go to.

    The init is built "from scratch" with busybox which allows us easier
    customization as well as much faster build time than using dracut.
    """
    busybox_cmds = [
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
    ln_bb = "\n".join(["ln -s busybox {}".format(cmd) for cmd in busybox_cmds])

    cp_modules = "\n".join([
        (
            "[ -f $(location {modules})/{mod} ] && " +
            "mkdir -p `dirname lib/modules/{uname}/kernel/{mod}` && " +
            "cp $(location {modules})/{mod} lib/modules/{uname}/kernel/{mod}"
        ).format(modules = modules, uname = uname, mod = mod)
        for mod in (
            "drivers/block/virtio_blk.ko",
            "fs/9p/9p.ko",
            "net/9p/9pnet.ko",
            "net/9p/9pnet_virtio.ko",
        )
    ])

    buck_genrule(
        name = name + "-tree",
        out = ".",
        cmd = """
        mkdir -p $OUT/bin
        cd $OUT

        cp $(location //fs_image/vm:init.sh) init

        {cp_modules}

        cp $(location {busybox}) bin/busybox
        pushd bin
        {ln_bb}
        """.format(
            ln_bb = ln_bb,
            cp_modules = cp_modules,
            busybox = third_party.library("busybox", "bin/busybox"),
        ),
        fs_image_internal_rule = True,
    )
    buck_genrule(
        name = name,
        out = name,
        cmd = """
        cd $(location :{name}-tree)
        find * | cpio -ovc 2>/dev/null | gzip > $OUT
        """.format(name = name),
    )
