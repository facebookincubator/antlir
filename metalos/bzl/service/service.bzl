load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

METALOS_PATH = "metalos"

# this is an helper that returns an antlir layer that matches the MetalOS native
# service specifications in antlir/docs/metalos/native-services
#
# - name is the name of the layer
# - service_name is the name of the service
# - binary is the binary of the service, could either be a buck rule or a file
# - parent_layer is the parent layer we want to inherit from, defaults to metalos.layer.base if not provided
# - service_binary_path is the path in the layer where the binary should be installed
# - user and group are the unix groups
# - visibility is a list of path that can use this macro, defaults to //metalos/... and //netos/...
def native_service(
        name,
        service_name,
        systemd_service_unit,
        binary,
        parent_layer = None,
        service_binary_path = None,
        user = "root",
        group = "root",
        visibility = None):
    if not service_binary_path:
        service_binary_path = "/bin/{}".name

    if not visibility:
        visibility = ["//metalos//...", "//netos/..."]

    if not parent_layer:
        parent_layer = REPO_CFG.artifact["metalos.layer.base"]

    image.layer(
        name = name,
        parent_layer = parent_layer,
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = visibility,
        features = [
            image.ensure_subdirs_exist(
                "/",
                METALOS_PATH,
                user = user,
                group = group,
                mode = 0o0770,
            ),
            feature.install(
                binary,
                service_binary_path,
                mode = "a+rx",
            ),
            feature.install(
                systemd_service_unit,
                "/{}/{}.service".format(METALOS_PATH, service_name),
            ),
        ],
    )
