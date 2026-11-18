# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")

_LINUX_TRIPLES = {
    "prelude//cpu:arm64": "aarch64-unknown-linux-gnu",
    "prelude//cpu:x86_64": "x86_64-unknown-linux-gnu",
}

def download_rust_toolchain(name: str, version: str, date: str, checksums: dict, visibility: list[str]):
    components = []
    for component in ["rustc", "rust-std", "clippy"]:
        archive_name = name + "-" + component
        native.http_archive(
            name = archive_name,
            urls = select({
                cpu: ["https://static.rust-lang.org/dist/{}/{}-{}-{}.tar.xz".format(date, component, version, triple)]
                for cpu, triple in _LINUX_TRIPLES.items()
            }),
            sha256 = select({cpu: checksums[triple][component] for cpu, triple in _LINUX_TRIPLES.items()}),
            strip_prefix = select({
                cpu: "{}-{}-{}/{}".format(
                    component,
                    version,
                    triple,
                    "rust-std-" + triple if component == "rust-std" else "clippy-preview" if component == "clippy" else component,
                )
                for cpu, triple in _LINUX_TRIPLES.items()
            }),
            target_compatible_with = ["prelude//os:linux"],
        )
        components.append(":" + archive_name)

    rust_toolchain(
        name = name,
        components = components,
        target_triple = select(_LINUX_TRIPLES),
        target_compatible_with = ["prelude//os:linux"],
        visibility = visibility,
    )

def _rust_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    # rust-dist components share an installation prefix. Keep the compiler's
    # shared libraries and the standard library beside the executables so that
    # rustc, rustdoc, and Clippy can find them without rustup or host Rust.
    sysroot = ctx.actions.declare_output("sysroot", dir = True)
    ctx.actions.run(
        cmd_args(
            "sh",
            "-c",
            'set -eu; sysroot="$1"; shift; mkdir -p "$sysroot"; for component in "$@"; do cp -a "$component"/. "$sysroot"/; done',
            "install-rust",
            sysroot.as_output(),
            ctx.attrs.components,
        ),
        category = "rust_toolchain",
    )

    tools = {
        name: RunInfo(args = cmd_args(sysroot.project("bin/" + binary), hidden = [sysroot]))
        for name, binary in {
            "clippy": "clippy-driver",
            "rustc": "rustc",
            "rustdoc": "rustdoc",
        }.items()
    }
    return [
        DefaultInfo(
            default_output = sysroot,
            sub_targets = {name: [DefaultInfo(default_output = sysroot), tool] for name, tool in tools.items()},
        ),
        RustToolchainInfo(
            compiler = tools["rustc"],
            clippy_driver = tools["clippy"],
            rustdoc = tools["rustdoc"],
            default_edition = "2024",
            nightly_features = True,
            panic_runtime = PanicRuntime("unwind"),
            rustc_target_triple = ctx.attrs.target_triple,
            sysroot_path = sysroot,
        ),
    ]

rust_toolchain = rule(
    impl = _rust_toolchain_impl,
    attrs = {
        "components": attrs.list(attrs.source()),
        "target_triple": attrs.string(),
    },
    is_toolchain_rule = True,
)
