load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//metalos/os/tests:defs.bzl", "systemd_expectations_test")

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
# - extra_features is used to personalise your layer
def native_service(
        name,
        service_name,
        systemd_service_unit,
        binary,
        parent_layer = None,
        service_binary_path = None,
        user = "root",
        group = "root",
        generator_binary = None,
        extra_features = [],
        visibility = None):
    if not service_binary_path:
        service_binary_path = "/usr/bin/{}".format(service_name)

    if not visibility:
        visibility = ["//metalos//...", "//netos/..."]

    if not parent_layer:
        parent_layer = REPO_CFG.artifact["metalos.layer.base"]

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
    ]

    if generator_binary:
        features.append(feature.install(generator_binary, "/metalos/generator", mode = "a+rx"))

    features.extend(extra_features)

    image.layer(
        name = name,
        parent_layer = parent_layer,
        flavor = REPO_CFG.antlir_linux_flavor,
        visibility = visibility,
        features = features,
    )

    _generate_systemd_expectations_test(name, service_name, systemd_service_unit, visibility)

def _generate_systemd_expectations_test(layer_name, service_name, systemd_service_unit, visibility):
    service_name_t = shape.shape(service_name = str)
    service_name_instance = shape.new(
        service_name_t,
        service_name = service_name,
    )

    # because the service is installed in /metalos/<service_name>.service and not in a standard
    # systemd path we need to create another layer where we install the unit so it will be
    # visible to systemd and the systemd_expectations_test
    image.layer(
        name = "{}-native-service-systemd-expectations".format(layer_name),
        parent_layer = ":{}".format(layer_name),
        features = [
            systemd.install_unit(systemd_service_unit, "{}.service".format(service_name)),
            # we do not want the unit to start but we still want to analyse it
            systemd.install_dropin("//metalos/os/tests:skip-unit.conf", "{}.service".format(service_name)),
            systemd.enable_unit("{}.service".format(service_name), "multi-user.target"),
        ],
    )
    systemd_expectations_rendered_template = shape.render_template(
        name = "systemd-expectations-rendered-template",
        instance = service_name_instance,
        template = "//metalos/bzl/service:systemd-expectations-template",
    )
    systemd_expectations_test(
        name = "systemd-expectations",
        expectations = systemd_expectations_rendered_template,
        layer = ":{}-native-service-systemd-expectations".format(layer_name),
    )
