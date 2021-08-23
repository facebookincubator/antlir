load(
    "//antlir/bzl:oss_shim.bzl",
    "buck_genrule",
    "http_archive",
    "rust_binary",
    "rust_library",
)

def archive(name, sha256, url):
    http_archive(
        name = name,
        urls = [url],
        sha256 = sha256,
        type = "tar.gz",
    )

def _extract_file(archive, src):
    name = archive[1:] + "/" + src
    if not native.rule_exists(name):
        buck_genrule(
            name = "{}/{}".format(archive[1:], src),
            out = src,
            cmd = "mkdir -p `dirname $OUT`; cp $(location {})/{} $OUT".format(archive, src),
        )
    return ":" + name

def third_party_rust_library(name, archive, srcs, mapped_srcs = None, **kwargs):
    kwargs["unittests"] = False
    if archive:
        src_targets = {}
        for src in srcs:
            src_targets[_extract_file(archive, src)] = src

        for target, src in mapped_srcs.items():
            src_targets[extract_buildscript_src(target)] = src
        rust_library(
            name = name,
            srcs = [],
            mapped_srcs = src_targets,
            **kwargs
        )
    else:
        rust_library(
            name = name,
            srcs = srcs,
            mapped_srcs = mapped_srcs,
            **kwargs
        )

def _get_native_host_triple():
    return "x86_64-unknown-linux-gnu"

# Invoke something with a default cargo-like environment. This is used to invoke buildscripts
# from within a Buck rule to get it to do whatever it does (typically, either emit command-line
# options for rustc, or generate some source).
def _make_preamble(
        out_dir,
        package_name,
        version,
        features,
        cfgs,
        env,
        target_override):
    # Work out what rustc to pass to the script
    rustc = native.read_config("rust", "compiler", "rustc")
    if "//" in rustc:
        rustc = "$(exe %s)" % rustc

    # CWD of a genrule script is the source directory but use $SRCDIR to make it an absolute path
    return """
        mkdir -p {out_dir}; \
        env \
            CARGO_MANIFEST_DIR=$SRCDIR/vendor/{package_name}-{version} \
            RUST_BACKTRACE=1 \
            OUT_DIR={out_dir} \
            CARGO=/bin/false \
            {features} \
            {cfgs} \
            CARGO_PKG_NAME={package_name} \
            CARGO_PKG_VERSION={version} \
            TARGET={target} \
            HOST={host} \
            RUSTC={rustc} \
            RUSTC_LINKER=/bin/false \
            `{rustc} --print cfg | awk -f $(location //third-party/rust/tools:cargo_cfgs.awk)` \
            {env} \
    """.format(
        out_dir = out_dir,
        package_name = package_name,
        version = version,
        features = " ".join(
            [
                "CARGO_FEATURE_{}=1".format(feature.upper().replace("-", "_"))
                for feature in features or []
            ],
        ),
        cfgs = " ".join(
            [
                "CARGO_CFG_{}=1".format(cfg.upper().replace("-", "_"))
                for cfg in cfgs or []
            ],
        ),
        target = target_override or _get_native_host_triple(),
        host = _get_native_host_triple(),
        rustc = rustc,
        env = "\\\n".join(
            ["'{}'='{}'".format(var, val) for var, val in (env or {}).items()],
        ),
    )

def _is_buildscript(crate, crate_root):
    return crate == "build_script_build" or crate_root.endswith("build.rs") or crate_root.endswith("build/main.rs")

def third_party_rust_binary(name, archive, srcs, mapped_srcs = None, **kwargs):
    kwargs.pop("proc_macro")
    kwargs["unittests"] = False

    if archive:
        src_targets = {}
        for src in srcs:
            src_targets[_extract_file(archive, src)] = src

        for target, src in mapped_srcs.items():
            src_targets[extract_buildscript_src(target)] = src
        rust_binary(
            name = name,
            srcs = [],
            mapped_srcs = src_targets,
            **kwargs
        )
    else:
        rust_binary(
            name = name,
            srcs = srcs,
            mapped_srcs = mapped_srcs,
            **kwargs
        )

    if _is_buildscript(kwargs["crate"], kwargs["crate_root"]):
        pre = _make_preamble(
            "\\$(dirname $OUT)",
            kwargs.get("crate", name),
            kwargs.get("version", ""),
            kwargs.get("features", []),
            kwargs.get("cfgs", []),
            None,
            None,
        )
        buck_genrule(
            name = name + "-args",
            out = "args",
            cmd = pre + "$(exe :{}) | $(exe //third-party/rust/tools:buildrs-rustc-flags) --filter > $OUT".format(name),
        )

        pre = _make_preamble(
            "$OUT",
            kwargs.get("crate", name),
            kwargs.get("version", ""),
            kwargs.get("features", []),
            kwargs.get("cfgs", []),
            None,
            None,
        )
        buck_genrule(
            name = name + "-srcs",
            out = ".",
            cmd = "mkdir -p $OUT; cd $OUT;" + pre + "$(exe :{})".format(name),
        )

def extract_buildscript_src(target):
    buildscript_srcs, src = target.rsplit("=", 1)
    if not buildscript_srcs.startswith("//third-party/rust:"):
        fail("buildscript-srcs must start with //third-party/rust:")
    buildscript_srcs = buildscript_srcs[len("//third-party/rust:"):]
    buck_genrule(
        name = buildscript_srcs + "=" + src,
        out = "unused",
        cmd = "cp $(location :{})/{} $OUT".format(buildscript_srcs, src),
    )
    return ":" + buildscript_srcs + "=" + src
