load("//antlir/bzl:shape.bzl", "shape")

container_unittest_opts_t = shape.shape(
    boot = shape.field(bool, default = False),
    layer = shape.target(
        default = "//metalos/os:metalos",
    ),
)

unittest_opts_t = shape.shape(
    container = shape.field(container_unittest_opts_t, optional = True),
)
