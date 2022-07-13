load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
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
        feature.ensure_dirs_exist(METALOS_DIR),
        feature.ensure_subdirs_exist(METALOS_DIR, "bin"),
    ]
    if service.exec_info.runas.user != "root":
        user_home_dir = "/home/{}".format(service.exec_info.runas.user)
        features.append(feature.setup_standard_user(
            service.exec_info.runas.user,
            service.exec_info.runas.group,
            user_home_dir,
        ))

    # do some checks on service properties
    if service.exec_info.resource_limits:
        if service.exec_info.resource_limits.open_fds and service.exec_info.resource_limits.open_fds < 0:
            fail("service.exec_info.resource_limits.open_fds must be a positive integer")
        if service.exec_info.resource_limits.memory_max_bytes and service.exec_info.resource_limits.memory_max_bytes < 0:
            fail("service.exec_info.resource_limits.memory_max_bytes must be a positive integer")

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
        parent_layer = "//metalos/services/base:base",
        visibility = visibility if visibility != None else ["//metalos/...", "//netos/..."],
    )
    # @oss-disable: native_service_fbpkg(name = service.name, layer = ":{}--layer".format(service.name)) 

def binary_target_to_path(target):
    return paths.join(METALOS_DIR, "bin/{}".format(target.replace("/", "."))).lstrip(".")
