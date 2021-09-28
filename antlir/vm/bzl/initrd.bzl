# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "get_visibility")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")

DEFAULT_MODULE_LIST = [
    "drivers/block/virtio_blk.ko",
    "drivers/block/loop.ko",
    "drivers/char/hw_random/virtio-rng.ko",
    "drivers/net/net_failover.ko",
    "drivers/net/virtio_net.ko",
    "fs/9p/9p.ko",
    "net/9p/9pnet.ko",
    "net/9p/9pnet_virtio.ko",
    "net/core/failover.ko",
    "drivers/nvme/host/nvme.ko",
    "drivers/nvme/host/nvme-core.ko",
]

def initrd(kernel, module_list = None, visibility = None):
    """
    Construct an initrd (gzipped cpio archive) that can be used to boot this
    kernel in a virtual machine and setup the root disk as a btrfs seed device
    with the second disk for writes to go to.

    The init is built "from scratch" with busybox which allows us easier
    customization as well as much faster build time than using dracut.
    """

    name = "{}-initrd".format(kernel.uname)
    module_list = module_list or DEFAULT_MODULE_LIST
    visibility = get_visibility(visibility, name)

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
            where = "/rootdisk/usr/lib/modules/{}".format(kernel.uname),
            type = "9p",
            options = ["ro", "trans=virtio", "version=9p2000.L", "cache=loose", "posixacl"],
        ),
    )
    mount_unit_name = systemd.escape("/rootdisk/usr/lib/modules/{}.mount".format(kernel.uname), path = True)

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

    # Build an initrd specifically for operating as a VM. This is built on top of the
    # MetalOS initrd and modified to support btrfs seed devices and 9p shared mounts
    # for the repository, kernel modules, and others.
    image.layer(
        name = name + "--layer",
        parent_layer = "//metalos/initrd:base",
        features = [
            # The metalctl generator will instantiate this template with the
            # seed device provided on the kernel command line as metalos.seed_device.
            systemd.install_unit("//antlir/vm/initrd:seedroot-device-add@.service"),
            systemd.install_unit("//antlir/vm/initrd:seedroot.service"),
            systemd.enable_unit("seedroot.service", target = "initrd-fs.target"),

            # The switchroot behavior is different for the vmtest based initrd so
            # lets remove the metalos-switch-root.service and install our own
            feature.remove("/usr/lib/systemd/system/metalos-switch-root.service"),
            feature.remove("/usr/lib/systemd/system/initrd-switch-root.target.requires/metalos-switch-root.service"),
            systemd.install_unit("//antlir/vm/initrd:initrd-switch-root.service"),
            systemd.enable_unit("initrd-switch-root.service", target = "initrd-switch-root.target"),

            # mount kernel modules over 9p in the initrd so they are available
            # immediately in the base os.
            systemd.install_unit(":" + name + "--modules.mount", mount_unit_name),
            systemd.enable_unit(mount_unit_name, target = "initrd-fs.target"),

            # Install the initrd modules specified in VM_MODULE_LIST above into the
            # layer
            image.ensure_subdirs_exist("/usr/lib", paths.join("modules", kernel.uname)),
            image.install(
                image.source(
                    source = ":" + name + "--modules",
                    path = ".",
                ),
                paths.join("/usr/lib/modules", kernel.uname, "kernel"),
            ),
            [
                [
                    image.clone(
                        kernel.artifacts.modules,
                        paths.join("/modules.{}".format(f)),
                        paths.join("/usr/lib/modules", kernel.uname, "modules.{}".format(f)),
                    ),
                    image.clone(
                        kernel.artifacts.modules,
                        paths.join("/modules.{}.bin".format(f)),
                        paths.join("/usr/lib/modules", kernel.uname, "modules.{}.bin".format(f)),
                    ),
                ]
                for f in ("dep", "symbols", "alias", "builtin")
            ],

            # Ensure the kernel modules are loaded by systemd when the initrd is started
            image.ensure_subdirs_exist("/usr/lib", "modules-load.d"),
            image.install(":" + name + "--modules-load.conf", "/usr/lib/modules-load.d/initrd-modules.conf"),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = [],
    )

    package.new(
        name = name,
        layer = ":" + name + "--layer",
        format = "cpio.gz",
        visibility = visibility,
    )

    # Build the debug version of the initrd by the cpio concat method.
    # TODO: Refactor this to not use the concat method since we aren't going
    # to be using that in production.
    buck_genrule(
        name = name + "-debug",
        out = "initrd.cpio.gz",
        cmd = "cat $(location :{}) $(location //metalos/initrd/debug:debug-append.cpio.gz) > $OUT".format(name),
        antlir_rule = "user-internal",
        visibility = visibility,
    )
