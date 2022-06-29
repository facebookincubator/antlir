load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
# @oss-disable: load("//metalos/bzl/service/facebook:service_fbpkg.bzl", "native_service_fbpkg") 

METALOS_DIR = "/metalos"

# Create an image and an fbpkg for a MetalOS native service defined in a
# service_t shape (from service.shape.bzl)
def native_service(
        service,
        extra_features = None,
        visibility = None):
    features = [
        image.ensure_dirs_exist(METALOS_DIR),
        image.ensure_subdirs_exist(METALOS_DIR, "bin"),
    ]
    if service.exec_info.runas.user != "root":
        user_home_dir = "/home/{}".format(service.exec_info.runas.user)
        features.append(feature.setup_standard_user(
            service.exec_info.runas.user,
            service.exec_info.runas.group,
            user_home_dir,
        ))

    # install buck binaries at a path based on their target so that the user
    # doesn't have to provide a unique name that would then have to be
    # propagated to the native service lib that writes out the unit file
    binaries = {
        binary_target_to_path(cmd.binary): cmd.binary
        for cmd in service.exec_info.pre + service.exec_info.run
        if ":" in cmd.binary
    }
    for cmd in service.exec_info.pre + service.exec_info.run:
        if ":" in cmd.binary and "//" not in cmd.binary:
            fail("all binaries used in native services must be using absolute target paths ({})".format(cmd.binary))
    features.extend([
        feature.install_buck_runnable(
            src,
            dst,
            user = service.exec_info.runas.user,
            group = service.exec_info.runas.group,
        )
        for dst, src in binaries.items()
    ])
    features.extend([
        feature.install_buck_runnable(
            src,
            dst,
            user = service.exec_info.runas.user,
            group = service.exec_info.runas.group,
        )
        for dst, src in binaries.items()
    ])

    if service.config_generator:
        features.append(feature.install(service.config_generator, "/metalos/generator", mode = "a+rx"))

    features.append(__DELETED_IN_STACK_gen_unit(service))

    buck_genrule(
        name = "{}--binary-thrift".format(service.name),
        cmd = "echo {} | $(exe //metalos/bzl/service:serialize-shape) > $OUT".format(shell.quote(shape.do_not_cache_me_json(service))),
        antlir_rule = "user-internal",
    )
    features.append(feature.install(":{}--binary-thrift".format(service.name), "/metalos/service.shape"))

    if extra_features:
        features.extend(extra_features)

    image.layer(
        name = service.name + "--layer",
        features = features,
        parent_layer = REPO_CFG.artifact["metalos.layer.base"],
        visibility = visibility if visibility != None else ["//metalos/...", "//netos/..."],
    )
    # @oss-disable: native_service_fbpkg(name = service.name, layer = ":{}--layer".format(service.name)) 

# this will be deleted later in this diff stack when metalos natively
# understands the service shape, and exists to break up this feature into two
# smaller diffs, one that implements the build-time interface and another on the
# runtime side
def __DELETED_IN_STACK_gen_unit(service):
    unit = "[Unit]\n"
    for dep in service.dependencies:
        if dep.mode == "after-only":
            unit += "After={}\n".format(dep.unit)
        if dep.mode == "requires-only":
            unit += "Requires={}\n".format(dep.unit)
        if dep == "requires-and-after":
            unit += "Requires={}\n".format(dep.unit)
            unit += "After={}\n".format(dep.unit)

    unit += "\n[Service]\n"
    unit += "Type={}\n".format(service.exec_info.service_type)
    unit += "User={}\n".format(service.exec_info.runas.user)
    unit += "Group={}\n".format(service.exec_info.runas.group)
    for cmd in service.exec_info.pre:
        unit += "ExecStartPre={}\n".format(exec_line(cmd))
    for cmd in service.exec_info.run:
        unit += "ExecStart={}\n".format(exec_line(cmd))
    for key, val in service.exec_info.environment.items():
        unit += "Environment={}={}\n".format(key, val)
    if service.exec_info.restart:
        unit += "Restart={}\n".format(service.exec_info.restart)
    if service.exec_info.resource_limits:
        if service.exec_info.resource_limits.open_fds:
            unit += "LimitNOFILE={}\n".format(service.exec_info.resource_limits.open_fds)
        if service.exec_info.resource_limits.memory_max_bytes:
            unit += "MemoryMax={}\n".format(service.exec_info.resource_limits.memory_max_bytes)

    if service.certificates and service.certificates.needs_service_cert:
        unit += "BindReadOnlyPaths=/var/facebook/x509_svc/{}_server.pem\n".format(service.name)
        unit += "BindReadOnlyPaths=/var/facebook/x509_svc/{}_client.pem\n".format(service.name)
        unit += "Environment=THRIFT_TLS_SRV_CERT=/var/facebook/x509_svc/{}_server.pem\n".format(service.name)
        unit += "Environment=THRIFT_TLS_SRV_KEY=/var/facebook/x509_svc/{}_server.pem\n".format(service.name)
        unit += "Environment=THRIFT_TLS_CL_CERT=/var/facebook/x509_svc/{}_client.pem\n".format(service.name)
        unit += "Environment=THRIFT_TLS_CL_KEY=/var/facebook/x509_svc/{}_client.pem\n".format(service.name)
    if service.certificates and service.certificates.needs_host_cert:
        unit += "BindReadOnlyPaths=/etc/host_client.pem\n"
        unit += "BindReadOnlyPaths=/etc/host_server.pem\n"

    buck_genrule(
        name = "{}--service-unit".format(service.name),
        cmd = "echo {} > $OUT".format(shell.quote(unit)),
        visibility = [],
        antlir_rule = "user-internal",
    )
    return feature.install(
        ":{}--service-unit".format(service.name),
        paths.join(METALOS_DIR, "{}.service".format(service.name)),
    )

def binary_target_to_path(target):
    return paths.join(METALOS_DIR, "bin/{}".format(target.replace("/", "."))).lstrip(".")

def exec_line(cmd):
    argv0 = cmd.binary
    if ":" in argv0:
        argv0 = binary_target_to_path(argv0)
    return "{} {}".format(argv0, " ".join([shell.quote(a) for a in cmd.args]))
