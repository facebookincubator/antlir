load("//fs_image/bzl:oss_shim.bzl", "buck_genrule", "third_party")

def initrd(name, modules = None):
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
        "ls",
        "mdev",
        "mkdir",
        "mount",
        "sh",
        "switch_root",
        "umount",
        "uname",
    ]
    ln_bb = "\n".join(["ln -s busybox {}".format(cmd) for cmd in busybox_cmds])

    cp_modules = "cp -R $(location {modules}) \"$OUT/modules\"".format(modules = modules) if modules else ""

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
    )
    buck_genrule(
        name = name,
        out = name,
        cmd = """
        cd $(location :{name}-tree)
        find * | cpio -ovc 2>/dev/null | gzip > $OUT
        """.format(name = name),
    )
