load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "third_party")

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
            mkdir -p $OUT
            pushd $OUT 2>/dev/null

            # copy the needed modules out of the module layer
            binary_path=( $(exe //antlir:find-built-subvol) )
            layer_loc="$(location {module_layer})"
            mod_layer_path=\\$( "${{binary_path[@]}}" "$layer_loc" )

            mods="{module_list}"
            for mod in $mods; do
                mod_src="$mod_layer_path/kernel/$mod"
                if [[ -f "$mod_src" ]]; then
                    mod_dir=\\$(dirname "$mod")
                    mkdir -p "$mod_dir"
                    cp "$mod_src" "$mod_dir"
                fi
            done
        """.format(
            module_layer = kernel.modules,
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
            image.clone(
                src_layer = ":seedroot",
                src_path = "/build/seedroot",
                dest_path = "/bin/seedroot",
            ),
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
