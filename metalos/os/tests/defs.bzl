load("//antlir/bzl:container_opts.shape.bzl", "container_opts_t")
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
        boot = True,
        run_as_user = "root",
        deps = ["//metalos/lib/systemd:systemd"] + third_party.libraries([
            "anyhow",
            "serde",
            "toml",
            "slog",
            "slog_glog_fmt",
            "tokio",
        ], platform = "rust"),
        container_opts = container_opts_t(boot_await_system_running = False),
    )

def skip_unit(unit):
    return systemd.install_dropin("//metalos/os/tests:99-skip-unit.conf", unit)
