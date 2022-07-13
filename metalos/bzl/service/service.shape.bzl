load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

binary_t = shape.union(target_t, str)

cmd_t = shape.shape(
    binary = binary_t,
    args = shape.list(str),
)

restart_mode_t = shape.enum("always")

resource_limits_t = shape.shape(
    # LimitNOFILE, open file descriptors allowed (ulimit -n)
    open_fds = shape.field(int, optional = True),
    # MemoryMax
    memory_max_bytes = shape.field(int, optional = True),
)

runas_t = shape.shape(
    user = shape.field(str, default = "root"),
    group = shape.field(str, default = "root"),
)

service_type_t = shape.enum("simple", "notify")

exec_t = shape.shape(
    runas = shape.field(runas_t, default = shape.new(runas_t)),
    pre = shape.field(shape.list(cmd_t), default = []),
    run = shape.field(shape.list(cmd_t), default = []),
    environment = shape.field(shape.dict(str, str), default = {}),
    restart = shape.field(restart_mode_t, optional = True),
    resource_limits = shape.field(resource_limits_t, optional = True),
    service_type = shape.field(service_type_t, default = "simple"),
)

dependency_mode_t = shape.enum("requires-and-after", "requires-only", "after-only")

dependency_t = shape.shape(
    unit = str,
    mode = shape.field(dependency_mode_t, default = "requires-and-after"),
)

service_cert_t = shape.shape(
    service_name = str,
)

certificates_t = shape.shape(
    # if True, MetalOS will bind the certificate files into the rootfs of the
    # container, and fully automate request + renewal
    service_cert = shape.field(service_cert_t, optional = True),
    # if True, the host certificate will be bind-mounted in
    needs_host_cert = shape.field(bool, default = False),
)

service_t = shape.shape(
    name = str,
    exec_info = exec_t,
    dependencies = shape.field(shape.list(dependency_t), default = []),
    config_generator = shape.field(target_t, optional = True),
    certificates = shape.field(certificates_t, optional = True),
)
