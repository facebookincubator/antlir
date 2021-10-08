# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def install_kernel_modules(kernel, module_list):
    # This intermediate genrule is here to create a dir hierarchy
    # of kernel modules that are needed for the initrd.  This
    # provides a single dir that can be cloned into the initrd
    # layer and allows for kernel modules that might be missing
    # from different kernel builds.
    buck_genrule(
        name = kernel.uname + "-selected--modules",
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
    )
    buck_genrule(
        name = kernel.uname + "selected--modules-load.conf",
        out = "unused",
        cmd = "echo '{}' > $OUT".format("\n".join([
            paths.basename(module).rsplit(".")[0]
            for module in module_list
        ])),
        antlir_rule = "user-internal",
        visibility = [],
    )
    return [
        # Install the kernel modules specified in module_list above into the
        # layer
        image.ensure_subdirs_exist("/usr/lib", paths.join("modules", kernel.uname)),
        feature.install(
            image.source(
                source = ":" + kernel.uname + "-selected--modules",
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
        feature.install(":" + kernel.uname + "selected--modules-load.conf", "/usr/lib/modules-load.d/initrd-modules.conf"),
    ]
