load("//antlir/bzl:shape.bzl", "shape")

kernel_artifacts_t = shape.shape(
    uname = str,
    devel = shape.layer(),
    modules = shape.layer(),
    vmlinuz = shape.layer(),
)

def make_kernel_artifacts(uname):
    name = "{}-artifacts".format(uname)
    shape.json_file(
        name = name,
        instance = shape.new(
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
        shape = kernel_artifacts_t,
    )

    return ":" + name
