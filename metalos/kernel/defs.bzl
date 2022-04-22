# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "http_file")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:defs.bzl", "package")
load(":kernel.shape.bzl", "derived_kernel_targets_t", "kernel_t", "upstream_kernel_targets_t")

__DEFAULT_VISIBILITY = ["//antlir/...", "//metalos/..."]

# All modules that are ever needed for disk boot (on any host). On some kernels
# these may be compiled in already, in which case they are skipped in the
# disk-boot-modules cpio generation. If any module is needed for disk boot
# (modules needed to mount the root disk or network drivers for the first boot
# where no actual images are on disk), it must be added to this list.
#
# TODO(T110770106) audit whether some of these can be removed (mainly 9p)
__DISK_BOOT_INITRD_MODULES = [
    "drivers/block/virtio_blk.ko",
    "drivers/block/loop.ko",
    "drivers/char/hw_random/virtio-rng.ko",
    "drivers/net/net_failover.ko",
    "drivers/net/virtio_net.ko",
    "fs/9p/9p.ko",
    # "net/9p/9pnet.ko",
    "net/9p/9pnet_virtio.ko",
    "net/core/failover.ko",
    "drivers/nvme/host/nvme.ko",
    "drivers/nvme/host/nvme-core.ko",
]

def _name(uname, artifact):
    return "{}--{}".format(uname, artifact)

def _target(uname, artifact, base = ""):
    return normalize_target(base + ":" + _name(uname, artifact))

def _derived_targets_shape(uname, base = ""):
    return shape.new(
        derived_kernel_targets_t,
        vmlinuz = _target(uname, "vmlinuz", base),
        modules_directory = _target(uname, "modules", base),
        disk_boot_modules = _target(uname, "disk-boot-modules", base),
        image = _target(uname, "image", base),
    )

def _derived_targets(uname, upstream_artifacts):
    buck_genrule(
        name = _name(uname, "rpm-contents"),
        out = ".",
        cmd = """
            set -ue -o pipefail
            rpm2cpio $(location {main_rpm}) | cpio -idm --directory "$OUT"
            # this is an abs symlink that ends up broken when the
            # rpm is unpacked
            rm "$OUT/boot/vmlinux-{uname}"
        """.format(main_rpm = upstream_artifacts.main_rpm, uname = uname),
        labels = ["uses_cpio"],
        visibility = [],
        antlir_rule = "user-internal",
    )
    buck_genrule(
        name = _name(uname, "vmlinuz"),
        cmd = "cp --reflink=auto $(location {rpm_contents})/boot/vmlinuz-{uname} $OUT".format(
            rpm_contents = _target(uname, "rpm-contents"),
            uname = uname,
        ),
        antlir_rule = "user-internal",
        visibility = __DEFAULT_VISIBILITY,
    )
    buck_genrule(
        name = _name(uname, "System.map"),
        cmd = "cp --reflink=auto $(location {rpm_contents})/boot/System.map-{uname} $OUT".format(
            rpm_contents = _target(uname, "rpm-contents"),
            uname = uname,
        ),
        antlir_rule = "user-internal",
    )
    buck_genrule(
        name = _name(uname, "modules"),
        cmd = """
            mkdir -p "$OUT"
            mkdir -p "$TMP/lib/modules"
            cp --reflink=auto --recursive \
                $(location {rpm_contents})/lib/modules/{uname} \
                "$TMP/lib/modules/"

            # run depmod here so that we can include the results in the layer we build
            # from this.
            depmod --basedir="$TMP" {uname}

            mv "$TMP/lib/modules/{uname}/"* "$OUT/"
        """.format(
            rpm_contents = _target(uname, "rpm-contents"),
            uname = uname,
        ),
        antlir_rule = "user-internal",
        visibility = __DEFAULT_VISIBILITY,
    )

    buck_genrule(
        name = _name(uname, "disk-boot-modules"),
        cmd = """
            mkdir -p "$OUT"
            mkdir -p "$TMP/lib/modules"

            mods="{module_list}"
            for mod in $mods; do
                mod_src="$(location {modules})/kernel/$mod"
                if [[ -f "$mod_src" ]]; then
                    mod_dir=\\$(dirname "$mod")
                    mkdir -p "$TMP/lib/modules/{uname}/kernel/$mod_dir"
                    cp "$mod_src" "$TMP/lib/modules/{uname}/kernel/$mod_dir"
                fi
            done

            # re-run depmod with just the disk boot modules to regenerate
            # modules.alias, modules.dep, etc
            depmod --basedir="$TMP" {uname}

            mv "$TMP/lib/modules/{uname}/"* "$OUT/"
        """.format(
            uname = uname,
            modules = _target(uname, "modules"),
            module_list = " ".join(__DISK_BOOT_INITRD_MODULES),
        ),
        visibility = __DEFAULT_VISIBILITY,
        antlir_rule = "user-internal",
    )

    image.layer(
        name = _name(uname, "disk-boot-modules-layer"),
        features = [
            image.ensure_dirs_exist("/usr/lib/modules"),
            feature.install(_target(uname, "disk-boot-modules"), paths.join("/usr/lib/modules", uname)),
            # If the devel headers/source are needed they will be bind mounted
            # into place on this directory, but this image is readonly so the
            # mountpoint must be created ahead of time
            image.ensure_subdirs_exist(paths.join("/usr/lib/modules", uname), "build"),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = [],
    )

    package.new(
        name = _name(uname, "disk-boot-modules.cpio.gz"),
        format = "cpio.gz",
        layer = _target(uname, "disk-boot-modules-layer"),
        visibility = [],
    )

    image.layer(
        name = _name(uname, "image"),
        features = [
            feature.install(_target(uname, "vmlinuz"), "/vmlinuz"),
            feature.install(_target(uname, "modules"), "/modules"),
            feature.install(_target(uname, "disk-boot-modules.cpio.gz"), "/disk-boot-modules.cpio.gz"),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = __DEFAULT_VISIBILITY,
    )

    return _derived_targets_shape(uname)

def _upstream_kernel_targets_shape(uname, base = "", has_devel = False, has_headers = False):
    return shape.new(
        upstream_kernel_targets_t,
        main_rpm = _target(uname, "main.rpm", base),
        devel_rpm = _target(uname, "devel.rpm", base) if has_devel else None,
        headers_rpm = _target(uname, "headers.rpm", base) if has_headers else None,
    )

def _kernel(kernel):
    uname = kernel.uname
    http_file(
        name = _name(uname, "main.rpm"),
        urls = [kernel.urls.main_rpm.url],
        sha256 = kernel.urls.main_rpm.sha256,
        visibility = ["PUBLIC"],
    )
    if hasattr(kernel.urls, "devel_rpm"):
        http_file(
            name = _name(uname, "devel.rpm"),
            urls = [kernel.urls.devel_rpm.url],
            sha256 = kernel.urls.devel_rpm.sha256,
            visibility = ["PUBLIC"],
        )
    if hasattr(kernel.urls, "headers_rpm"):
        http_file(
            name = _name(uname, "headers.rpm"),
            urls = [kernel.urls.headers_rpm.url],
            sha256 = kernel.urls.headers_rpm.sha256,
            visibility = ["PUBLIC"],
        )

    upstream = _upstream_kernel_targets_shape(
        uname = uname,
        base = "",
        has_devel = hasattr(kernel.urls, "devel_rpm"),
        has_headers = hasattr(kernel.urls, "headers_rpm"),
    )
    derived = _derived_targets(uname, upstream)
    return shape.new(
        kernel_t,
        uname = uname,
        upstream_targets = upstream,
        derived_targets = derived,
    )

def _pre_instantiated_kernel(uname):
    """
    Return a kernel_t from a uname that must have already been built with
    metalos_kernel.kernel()
    """
    return shape.new(
        kernel_t,
        uname = uname,
        upstream_targets = _upstream_kernel_targets_shape(
            uname,
            base = "//kernel/kernels",
            # we don't actually know if these targets exist, but the graph will
            # obviously fail if they don't so be optimistic
            has_devel = True,
            has_headers = True,
        ),
        derived_targets = _derived_targets_shape(uname, "//kernel/kernels"),
    )

metalos_kernel = struct(
    kernel = _kernel,
    pre_instantiated_kernel = _pre_instantiated_kernel,
)
