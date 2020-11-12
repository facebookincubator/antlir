load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")

kernel_artifacts_t = shape.shape(
    uname = str,
    devel = shape.layer(),
    modules = shape.layer(),
    vmlinuz = shape.layer(),
)

def make_kernel_artifacts(uname):
    name = "{}-artifacts".format(uname)

    # TODO: this is not totally on board with the new shape target requirements
    # from D24848531 yet
    buck_genrule(
        name = name,
        out = "unused.json",
        cmd = "echo {} > $OUT".format(
            shell.quote(shape.do_not_cache_me_json(
                shape.new(
                    kernel_artifacts_t,
                    uname = uname,
                    **{
                        artifact: "//{package}:{uname}-{artifact}".format(
                            # We need to provide the package because these target paths are
                            # used as a lookup to find the on-disk artifact from a completely
                            # different place.  So we need an absolute target, not a relative
                            # one.
                            package = native.package_name(),
                            uname = uname,
                            artifact = artifact,
                        )
                        for artifact in ["devel", "modules", "vmlinuz"]
                    }
                ),
                kernel_artifacts_t,
            )),
        ),
    )

    return ":" + name
