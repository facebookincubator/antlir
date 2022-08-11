load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "third_party")
load("//antlir/bzl:systemd.bzl", "systemd")

def systemd_expectations_test(name, layer, expectations):
    image.layer(
        name = name + "--layer",
        parent_layer = layer,
        features = [
            skip_unit("systemd-networkd-wait-online.service"),
        ],
    )
    image.rust_unittest(
        name = name,
        layer = ":{}--layer".format(name),
        mapped_srcs = {
            "//metalos/os/tests:systemd_expectations.rs": "systemd_expectations.rs",
            expectations: "expectations.toml",
        },
        crate_root = "systemd_expectations.rs",
        run_as_user = "root",
        deps = ["//metalos/lib/systemd:systemd"] + third_party.libraries([
            "anyhow",
            "serde",
            "serde_json",
            "toml",
        ], platform = "rust"),
    )

def skip_unit(unit):
    return [
        systemd.install_dropin("//metalos/os/tests:99-skip-unit.conf", unit, force = True),
    ]

def metalos_container_test_layer(
        name):
    image.layer(
        name = name,
    )
