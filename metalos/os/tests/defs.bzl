load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "third_party")

def analyze_all_units(name, layer, expected_problems = None):
    if not expected_problems:
        expected_problems = "//antlir:empty"

    image.rust_unittest(
        name = name,
        layer = layer,
        mapped_srcs = {
            "//metalos/os/tests:analyze_all_units.rs": "analyze_all_units.rs",
            expected_problems: "expected-problems.toml",
        },
        crate_root = "analyze_all_units.rs",
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
    )
