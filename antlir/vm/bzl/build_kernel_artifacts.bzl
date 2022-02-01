# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":kernel.shape.bzl", "kernel_artifacts_t", "kernel_t")

def build_kernel_artifacts(uname, devel_rpm, headers_rpm, rpm_exploded, include_vmlinux = True):
    """
    Build the set of kernel artifact targets needed for `antlir.vm`.  This returns an instance
    of the `kernel_t` shape.
    """

    # Install the devel rpm and headers rpm into a layer.
    # The reasons for this instead of using the same
    # pattern as the `rpm-exploded` targets are:
    #  - The devel rpm contains some internally consistent symlinks that
    #    we'd like to preserve when creating the image layer.  Currently
    #    the only way to do that is via the `image.clone` operation, which
    #    requires the source of the clone to be a layer.
    #  - The destination of the contents need to be installed at the root
    #    of the image layer (./).  This is currently not possible with the
    #    implementation of `image.source` since `./` ends up conflicting
    #    with the always provided /.
    image.layer(
        name = "{uname}--devel-headers-installed".format(uname = uname),
        # This is used because we need the gpg keys that this rpm is signed
        # by and the build appliance should have it.
        parent_layer = REPO_CFG.flavor_to_config[REPO_CFG.antlir_linux_flavor].build_appliance,
        features = [
            image.rpms_install([devel_rpm, headers_rpm]),
        ],
        visibility = [],
    )
    image.layer(
        name = "{}-devel".format(uname),
        features = [
            image.clone(
                ":{}--devel-headers-installed".format(uname),
                "usr/src/kernels/{}/".format(uname),
                "./",
            ),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = ["PUBLIC"],
    )

    image.layer(
        name = "{}-headers".format(uname),
        features = [
            image.clone(
                ":{}--devel-headers-installed".format(uname),
                "usr/include/",
                "./",
            ),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = ["PUBLIC"],
    )

    # This will extract all of the modules from the `{uname}-rpm-exploded` target as
    # well as any additional modules that aren't part of the kernel rpm (for older
    # kernels that weren't built with certain modules originally).
    # Then it will run depmod to generate the module dependency information
    # required.
    buck_genrule(
        name = "{uname}--precursor-of-modules".format(uname = uname),
        out = ".",
        cmd = """
            mkdir -p "$OUT/lib/modules/{uname}"
            cd "$OUT"

            cp --reflink=auto -R "$(location {rpm_exploded})/lib/modules/{uname}"/* "lib/modules/{uname}/"

            # run depmod here so that we can include the results in the layer we build
            # from this.
            depmod --basedir="$OUT" {uname}

            # if vmlinux is just 'vmlinux', copy it to be uniquely identified by its uname
            if [ -f $(location {rpm_exploded})/lib/modules/{uname}/vmlinux ]; then
                cp $(location {rpm_exploded})/lib/modules/{uname}/vmlinux "lib/modules/{uname}/vmlinux-{uname}"
            fi
        """.format(
            uname = uname,
            rpm_exploded = rpm_exploded,
        ),
        visibility = [],
        antlir_rule = "user-internal",
    )

    # The modules are inserted into the layer at the root
    # of the layer with the expectation that the layer
    # will be mounted for use at `/lib/modules/{uname}'.
    image.layer(
        name = "{}-modules".format(uname),
        features = [
            feature.install(
                image.source(
                    ":{}--precursor-of-modules".format(uname),
                    path = "lib/modules/{uname}/{part}".format(
                        uname = uname,
                        part = part,
                    ),
                ),
                part,
            )
            for part in [
                "kernel",  # The entire directory of modules
                # All the supporting metadata that modprobe and other
                # userspace tools need in order to deal with modules
                "modules.alias",
                "modules.alias.bin",
                "modules.builtin",
                "modules.builtin.bin",
                "modules.dep",
                "modules.dep.bin",
                "modules.devname",
                "modules.order",
                "modules.symbols",
                "modules.symbols.bin",
            ] + ([
                # Include the uncompressed kernel binary along with the modules so
                # that some bpf tools can use it.
                "vmlinux-{}".format(uname),
            ] if include_vmlinux else [])
        ] + [
            # If the devel headers/source are needed they will be
            # bind mounted into place on this directory. This is here
            # to support that.
            image.ensure_subdirs_exist("/", "build"),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = ["PUBLIC"],
    )

    # Return a new kernel_t instance
    return shape.new(
        kernel_t,
        uname = uname,
        artifacts = shape.new(
            kernel_artifacts_t,
            modules = ":{}-modules".format(uname),
            devel = ":{}-devel".format(uname),
            headers = ":{}-headers".format(uname),
            vmlinuz = ":{}-vmlinuz".format(uname),
        ),
    )
