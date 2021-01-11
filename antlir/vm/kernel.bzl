load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")

kernel_artifacts_t = shape.shape(
    vmlinuz = shape.target(),
    # devel and modules may not exist, such as in the case of a vmlinuz with
    # all necessary features compiled with =y
    devel = shape.layer(optional = True),
    modules = shape.layer(optional = True),
)

kernel_t = shape.shape(
    uname = str,
    artifacts = shape.field(kernel_artifacts_t),
)

def normalize_kernel(kernel):
    # Convert from a struct kernel struct format
    # into a kernel shape instance.  Note, if the provided `kernel` attr
    # is already a shape instance, this just makes another one. Wasteful, yes
    # but we don't have an `is_shape` mechanism yet to avoid something like
    # this.
    return shape.new(
        kernel_t,
        uname = kernel.uname,
        artifacts = shape.new(
            kernel_artifacts_t,
            devel = kernel.artifacts.devel,
            modules = kernel.artifacts.modules,
            vmlinuz = kernel.artifacts.vmlinuz,
        ),
    )

def build_kernel_artifacts(uname, devel_rpm, rpm_exploded, extra_modules = None, include_vmlinux = True):
    """
    Build the set of kernel artifact targets needed for `antlir.vm`.  This returns an instance
    of the `kernel_t` shape.
    """

    # Install the devel rpm into a layer.  The reasons for this instead of using the same
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
        name = "{uname}--devel-installed".format(uname = uname),
        # This is used because we need the gpg keys that this rpm is signed
        # by and the build appliance should have it.
        parent_layer = REPO_CFG.build_appliance_default,
        features = [
            image.rpms_install([devel_rpm]),
        ],
        visibility = [],
    )
    image.layer(
        name = "{}-devel".format(uname),
        features = [
            image.clone(
                ":{}--devel-installed".format(uname),
                "usr/src/kernels/{}/".format(uname),
                "./",
            ),
        ],
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

            {cp_extra_modules}

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
            # some older kernels were never built with the 9p fs module, so copy in
            # any modules that are checked in to fbcode that might be missing from
            # the rpm but are necessary for vmtest
            cp_extra_modules = """
                extra_mod_dir="$(location {extra_modules})/modules/{uname}"
                if [[ -d  "$extra_mod_dir" ]]; then
                    cp -R "$extra_mod_dir"/* "lib/modules/{uname}/kernel/"
                fi
            """.format(extra_modules = extra_modules, uname = uname) if extra_modules else "",
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
            image.install(
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
    )

    # Return a new kernel_t instance
    return shape.new(
        kernel_t,
        uname = uname,
        artifacts = shape.new(
            kernel_artifacts_t,
            modules = ":{}-modules".format(uname),
            devel = ":{}-devel".format(uname),
            vmlinuz = ":{}-vmlinuz".format(uname),
        ),
    )
