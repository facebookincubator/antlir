load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

# Get current target platform - hard-coded for example, matches one of the platforms
# defined in reindeer.toml.
def _get_plat():
    return "linux-x86_64"

# Matching host triple
def _get_native_host_triple():
    return "x86_64-unknown-linux-gnu"

def concat(*iterables):
    result = []
    for iterable in iterables:
        result.extend(iterable)
    return result

def extend(orig, new):
    if orig == None:
        ret = new
    elif new == None:
        ret = orig
    elif types.is_dict(orig):
        ret = orig.copy()
        ret.update(new)
    else:  # list
        ret = orig + new
    return ret

# Invoke something with a default cargo-like environment. This is used to invoke buildscripts
# from within a Buck rule to get it to do whatever it does (typically, either emit command-line
# options for rustc, or generate some source).
def _make_preamble(out_dir, package_name, version, features, cfgs, env, target_override):
    # Work out what rustc to pass to the script
    rustc = native.read_config("rust", "compiler", "rustc")
    if "//" in rustc:
        rustc = "(exe %s)" % rustc

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
            `{rustc} --print cfg | awk -f $(location //third-party/rust:cargo_cfgs.awk)` \
            {env} \
    """.format(
        out_dir = out_dir,
        package_name = package_name,
        version = version,
        features = " ".join(["CARGO_FEATURE_{}=1".format(feature.upper().replace("-", "_")) for feature in features or []]),
        cfgs = " ".join(["CARGO_CFG_{}=1".format(cfg.upper().replace("-", "_")) for cfg in cfgs or []]),
        target = target_override or _get_native_host_triple(),
        host = _get_native_host_triple(),
        rustc = rustc,
        env = "\\\n".join(["'{}'='{}'".format(var, val) for var, val in (env or {}).items()]),
    )

# Invoke a Rust buildscript binary with the right surrounding
# environment variables. `filters` is a shell command which takes the
# output of the build script and filters appropriately. It is given the
# final output file path on its commandline.
def rust_buildscript_genrule_filter(name, buildscript_rule, outfile, package_name, version, features = None, cfgs = None, env = None, target = None):
    pre = _make_preamble("\\$(dirname $OUT)", package_name, version, features, cfgs, env, target)
    native.cxx_genrule(
        name = name,
        out = outfile,
        cmd = pre + "$(exe {buildscript}) | $(location //third-party/rust:buildrs_rustc_flags.py) > $OUT".format(
            buildscript = buildscript_rule,
        ),
    )

# Invoke a build script for its generated sources.
def rust_buildscript_genrule_srcs(name, buildscript_rule, files, package_name, version, features = None, cfgs = None, env = None, target = None, srcs = None):
    pre = _make_preamble("$OUT", package_name, version, features, cfgs, env, target)
    native.cxx_genrule(
        name = name,
        out = name + "-outputs",
        srcs = srcs,
        cmd = pre + "$(exe {buildscript})".format(
            buildscript = buildscript_rule,
        ),
    )
    mainrule = ":" + name
    for file in files:
        native.cxx_genrule(
            name = "{}={}".format(name, file),
            out = file,
            cmd = "mkdir -p \\$(dirname $OUT) && cp $(location {main})/{file} $OUT".format(
                main = mainrule,
                file = file,
            ),
        )

# Add platform-specific args to args for a given platform. This assumes there's some static configuration
# for target platform (_get_plat) which isn't very flexible. A better approach would be to construct
# srcs/deps/etc with `select` to conditionally configure each target, but that's out of scope for this.
def platform_attrs(platformname, platformattrs, attrs):
    for attr in sets.to_list(sets.make(concat(attrs.keys(), platformattrs.get(platformname, {}).keys()))):
        new = extend(attrs.get(attr), platformattrs.get(platformname, {}).get(attr))
        attrs[attr] = new
    return attrs

def _archive_target_name(crate_root):
    if not crate_root.startswith("vendor/"):
        fail("expected '{}' to start with vendor/".format(crate_root), attr = "crate_root")
    crate_and_ver = crate_root[len("vendor/"):]
    crate_and_ver = crate_and_ver.split("/")[0]
    return crate_and_ver + "--archive"

def _extract_from_archive(archive, src):
    # some srcs are duplicated in the rust_library and rust_binary, so make
    # sure to only define it once
    if not native.rule_exists(src):
        native.genrule(
            name = src,
            out = "name-unused",
            cmd = "cp --reflink=auto $(location :{})/{} $OUT".format(archive, src),
        )
    return normalize_target(":" + src)

def third_party_rust_library(name, srcs, crate, crate_root, platform = {}, dlopen_enable = False, python_ext = None, mapped_srcs = None, **kwargs):
    if mapped_srcs:
        fail("mapped_srcs not yet supported", attr = "mapped_srcs")

    # Rust crates which are python extensions need special handling to make sure they get linked
    # properly. This is not enough on its own - it still assumes there's a dependency on the python
    # library.
    if dlopen_enable or python_ext:
        # This is all pretty ELF/Linux-specific
        linker_flags = ["-shared"]
        if python_ext:
            linker_flags.append("-uPyInit_{}".format(python_ext))
            kwargs["preferred_linkage"] = "static"
        native.cxx_binary(name = name + "-so", deps = [":" + name], link_style = "static_pic", linker_flags = linker_flags)

    # Download and extract the source tarball from crates.io with a genrule.
    # The python binary this is calling parses Cargo.lock to validate the
    # checksum of the downloaded archive, which would otherwise require a
    # second pass in buckification.

    archive_target = _archive_target_name(crate_root)
    native.genrule(
        name = archive_target,
        out = ".",
        cmd = "$(exe //third-party/rust:download) $(location //third-party/rust:Cargo.lock) {} {} $OUT".format(shell.quote(crate), shell.quote(crate_root)),
    )
    source_targets = {_extract_from_archive(archive_target, src): src for src in srcs}

    # ignore licenses for simplicity, they can be added back later if it becomes desirable
    kwargs.pop("licenses", None)

    native.rust_library(
        name = name,
        srcs = [],
        mapped_srcs = source_targets,
        crate = crate,
        crate_root = crate_root,
        **platform_attrs(_get_plat(), platform, kwargs)
    )

# `platform` is a map from a platform (defined in reindeer.toml) to the attributes
# specific to that platform.
def third_party_rust_binary(name, crate_root, srcs, platform = {}, mapped_srcs = None, **kwargs):
    if mapped_srcs:
        fail("mapped_srcs not yet supported", attr = "mapped_srcs")

    # The archive target will have been created by the third_party_rust_library
    # macro
    archive_target = _archive_target_name(crate_root)
    source_targets = {_extract_from_archive(archive_target, src): src for src in srcs}

    # ignore licenses for simplicity, they can be added back later if it becomes desirable
    kwargs.pop("licenses", None)

    native.rust_binary(
        name = name,
        crate_root = crate_root,
        mapped_srcs = source_targets,
        **platform_attrs(_get_plat(), platform, kwargs)
    )

def third_party_rust_cxx_library(name, **kwargs):
    native.cxx_library(name = name, **kwargs)

def third_party_rust_prebuilt_cxx_library(name, **kwargs):
    native.prebuilt_cxx_library(name = name, **kwargs)
