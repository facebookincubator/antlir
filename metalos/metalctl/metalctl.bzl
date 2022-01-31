load("//antlir/bzl:oss_shim.bzl", "third_party")
load("//antlir/bzl:shape.bzl", "shape")
load("//metalos:defs.bzl", "rust_binary")
load("//metalos:metalos_tests.shape.bzl", "container_unittest_opts_t", "unittest_opts_t")

def metalctl(name, rustc_flags = None, extra_deps = [], **kwargs):
    srcs = native.glob(["src/**/*.rs"])

    facebook = "src/facebook/mod.rs" in srcs

    # we don't yet have blkid support in oss
    have_blkid = third_party.library("util-linux", "blkid") != None

    # WARNING: these common_deps are included in both the initrd and rootfs builds
    # of metalctl. The size of the initrd is constrained and must remain small if we
    # ever want to pxe-boot it directly. Be prepared to justify any size increases
    # brought in by large dependencies.
    deps = [
        "//metalos/host_configs:evalctx",
        "//metalos/host_configs:package-manifest-rust",
        "//metalos/host_configs:host-rust",
        "//metalos/lib/btrfs:btrfs",
        "//metalos/lib/image:image",
        "//metalos/lib/systemd:systemd",
        "//metalos/lib:expand_partition",
        "//metalos/lib:find_root_disk",
        "//metalos/lib:generator_lib",
        "anyhow",  # ~9.5k, very helpful for error handling
        "nix",  # ~5k: access to syscalls (mount, etc)
        "libc",
        "structopt",  # ~300k, but makes iterating development much easier
        # all the slog crates together add about 50k
        "slog",
        "slog-async",
        "slog-term",
        "slog_glog_fmt",
        "toml",  # load config files
        "serde",  # load config files
        "serde_json",
        "maplit",  # Should be macros only so little to no difference in output binary
        "shlex",
        "strum",
        "strum_macros",  # I <3 zero cost abstractions!
        # Needed for HTTPS requests to download images
        "url",
        "bytes",
        "futures-core",
        "futures-util",
        "reqwest",
        "tokio",  # async runtime for http client
        "zstd",
    ] + extra_deps

    rustc_flags = rustc_flags or []
    if facebook:
        rustc_flags.append("--cfg=facebook")
        deps.append("//common/rust/fbwhoami:fbwhoami")
        deps.append("//common/rust/asset_gating/device:asset_gating_device")
    if have_blkid:
        rustc_flags.append("--cfg=blkid")
        deps.append("//metalos/lib/blkid:blkid")

    # metalctl is split into two binary targets, so that code that requires
    # features only found in the rootfs, or larger dependencies can be excluded
    # from the initrd.

    rust_binary(
        name = name,
        srcs = srcs,
        crate_root = "src/metalctl.rs",
        deps = deps,
        test_deps = [
            "mockall",
            "tempfile",
        ],
        test_srcs = native.glob(["tests/**/*.rs"]),
        unittest_opts = shape.new(
            unittest_opts_t,
            container = shape.new(
                container_unittest_opts_t,
                boot = True,
                layer = "//metalos/metalctl/tests/facebook:test-layer" if facebook else "//metalos/os:metalos",
            ),
        ),
        unittests = ["plain", "container", "vm"],
        rustc_flags = rustc_flags,
        **kwargs
    )
