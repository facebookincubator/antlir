# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:systemd.bzl", "systemd")

DEFAULT_MODULE_LIST = [
    "drivers/block/virtio_blk.ko",
    "fs/9p/9p.ko",
    "net/9p/9pnet.ko",
    "net/9p/9pnet_virtio.ko",
]

def initrd(kernel, module_list = None):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine and setup the root disk as a btrfs seed device
    with the second disk for writes to go to.

    The init is built "from scratch" with busybox which allows us easier
    customization as well as much faster build time than using dracut.
    """

    name = "{}-initrd".format(kernel.uname)

    module_list = module_list or DEFAULT_MODULE_LIST

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
            module_layer = kernel.artifacts.modules,
            module_list = " ".join(module_list),
        ),
        antlir_rule = "user-internal",
        visibility = [],
    )

    systemd.units.mount_file(
        name = name + "--modules.mount",
        mount = shape.new(
            systemd.units.mount,
            unit = shape.new(
                systemd.units.unit,
                description = "Full set of kernel modules",
                requires = ["seedroot.service", "systemd-modules-load.service"],
                after = ["seedroot.service", "systemd-modules-load.service"],
                before = ["initrd-fs.target"],
            ),
            what = "kernel-modules",
            where = "/sysroot/usr/lib/modules/{}".format(kernel.uname),
            type = "9p",
            options = ["ro", "trans=virtio", "version=9p2000.L", "cache=loose", "posixacl"],
        ),
    )
    mount_unit_name = systemd.escape("/sysroot/usr/lib/modules/{}.mount".format(kernel.uname), path = True)

    buck_genrule(
        name = name + "--modules-load.conf",
        out = "unused",
        cmd = "echo '{}' > $OUT".format("\n".join([
            paths.basename(module).rsplit(".")[0]
            for module in module_list
        ])),
        antlir_rule = "user-internal",
        visibility = [],
    )

    image.layer(
        name = name + "--layer",
        features = [
            image.ensure_dirs_exist("/usr/lib/systemd/system"),
            # mount /dev/vda at /sysroot, followed by seedroot units to make it
            # rw with /dev/vdb as a scratch device
            systemd.install_unit("//antlir/vm/initrd:sysroot.mount"),
            systemd.install_unit("//antlir/vm/initrd:seedroot-device-add.service"),
            systemd.install_unit("//antlir/vm/initrd:seedroot.service"),
            systemd.enable_unit("seedroot.service", target = "initrd-fs.target"),
            # mount kernel modules over 9p in the initrd so they are available
            # immediately in the base os
            systemd.install_unit(":" + name + "--modules.mount", mount_unit_name),
            systemd.enable_unit(mount_unit_name, target = "initrd-fs.target"),
            # load the initrd modules specified in VM_MODULE_LIST above
            image.ensure_dirs_exist("/usr/lib/modules-load.d"),
            image.install(":" + name + "--modules-load.conf", "/usr/lib/modules-load.d/initrd-modules.conf"),
            image.ensure_dirs_exist(paths.join("/usr/lib/modules", kernel.uname)),
            image.install(
                image.source(
                    source = ":" + name + "--modules",
                    path = ".",
                ),
                paths.join("/usr/lib/modules", kernel.uname, "kernel"),
            ),
            [
                image.clone(
                    kernel.artifacts.modules,
                    paths.join("/modules.{}.bin".format(f)),
                    paths.join("/usr/lib/modules", kernel.uname, "modules.{}.bin".format(f)),
                )
                for f in ("dep", "symbols", "alias", "builtin")
            ],
            image.ensure_dirs_exist("/usr/lib/systemd/system/systemd-tmpfiles-setup.service.d"),
            image.install("//antlir/vm/initrd:vmtest-tmpfiles-fix.conf", "/usr/lib/systemd/system/systemd-tmpfiles-setup.service.d/vmtest-tmpfiles-fix.conf"),
            image.ensure_dirs_exist("/usr/lib/systemd/system/systemd-tmpfiles-setup-dev.service.d"),
            image.install("//antlir/vm/initrd:vmtest-tmpfiles-fix.conf", "/usr/lib/systemd/system/systemd-tmpfiles-setup-dev.service.d/vmtest-tmpfiles-fix.conf"),
        ],
        visibility = [],
    )

    image.package(
        name = name + "--append.cpio.gz",
        layer = ":" + name + "--layer",
        format = "cpio.gz",
        visibility = [],
    )

    # Form the vmtest initrd by concatenating vmtest features to the base
    # Antlir Linux initrd
    buck_genrule(
        name = name,
        out = "initrd.cpio.gz",
        cmd = """
            cat \
                $(location //antlir/linux/bootloader:base.cpio.gz) \
                $(location :{}--append.cpio.gz) \
                > $OUT
            """.format(name),
        antlir_rule = "user-internal",
    )

    buck_genrule(
        name = name + "-debug",
        out = "initrd.cpio.gz",
        cmd = "cat $(location :{}) $(location //antlir/linux/bootloader/debug:debug-append.cpio.gz) > $OUT".format(name),
        antlir_rule = "user-internal",
    )
