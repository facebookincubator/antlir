load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

container_unittest_opts_t = shape.shape(
    boot = shape.field(bool, default = False),
    layer = shape.field(
        target_t,
        default = "//metalos/os:metalos",
    ),
)

unittest_opts_t = shape.shape(
    container = shape.field(container_unittest_opts_t, optional = True),
)
