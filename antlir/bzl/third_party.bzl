# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":constants.bzl", "REPO_CFG")
load(":hoist.bzl", "hoist")
load(":image.bzl", "image")
load(":oss_shim.bzl", "buck_genrule", third_party_shim = "third_party")
load(":third_party.shape.bzl", "dep_t", "script_t")

PREFIX = "/third_party_build"
SRC_TGZ = paths.join(PREFIX, "source.tar.gz")
SRC_DIR = paths.join(PREFIX, "src")
DEPS_DIR = paths.join(PREFIX, "deps")
STAGE_DIR = paths.join(PREFIX, "stage")
OUT_DIR = paths.join(PREFIX, "out")

def _cmd_prepare_dependency(dependency):
    """move the dependencies in the right places"""
    return "\n".join([
        "cp --reflink=auto -r {deps}/{name}/{path} {stage}".format(
            deps = DEPS_DIR,
            stage = STAGE_DIR,
            name = dependency.name,
            path = path,
        )
        for path in dependency.paths
    ])

def _native_build(name, base_features, script, deps = None):
    deps = deps or []

    prepare_deps = "\n".join([
        _cmd_prepare_dependency(dep)
        for dep in deps
    ])

    buck_genrule(
        name = name + "__build_script",
        out = "out",
        cmd = ("""
cat > $TMP/out << 'EOF'
# copy all specified dependencies
{prepare_deps}

# unpack the source in build dir
cd "{src_dir}"
tar xzf {src} --strip-components=1

export STAGE="{stage_dir}"
{prepare}
{build}

# trick the fs layer so that we can collect the installed files without
# dependencies mixed in; while keeping correct paths in pkg-config
mkdir {fswork_dir}
mv {stage_dir} {stage_ro_dir}
mkdir {stage_dir}
fuse-overlayfs -o lowerdir="{stage_ro_dir}",upperdir="{out_dir}",workdir={fswork_dir} "{stage_dir}"

{install}

# unmount the overlay and remove whiteout files because we only want the
# newly created ones by the install
fusermount -u "{stage_dir}"
find "{out_dir}" \\( -name ".wh.*" -o -type c \\) -delete
EOF
mv $TMP/out $OUT
chmod +x $OUT
        """).format(
            src = SRC_TGZ,
            prepare_deps = prepare_deps,
            prepare = script.prepare,
            build = script.build,
            install = script.install,
            src_dir = SRC_DIR,
            stage_dir = STAGE_DIR,
            stage_ro_dir = paths.join(PREFIX, "stage_ro"),
            fswork_dir = paths.join(PREFIX, "fswork"),
            out_dir = OUT_DIR,
        ),
    )

    image.layer(
        name = name + "__setup_layer",
        parent_layer = REPO_CFG.flavor_to_config[REPO_CFG.flavor_default].build_appliance,
        features = [
            image.ensure_dirs_exist(SRC_DIR),
            image.ensure_dirs_exist(DEPS_DIR),
            image.ensure_dirs_exist(STAGE_DIR),
            image.ensure_dirs_exist(OUT_DIR),
            feature.install(
                third_party_shim.source(name),
                SRC_TGZ,
            ),
            feature.install(
                ":" + name + "__build_script",
                "/build",
                mode = "a+x",
            ),
            image.rpms_install(["tar", "fuse", "fuse-overlayfs"]),
        ] + base_features + [
            feature.install(
                dep.source,
                paths.join(DEPS_DIR, dep.name),
            )
            for dep in deps
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
    )

    image.genrule_layer(
        name = name + "__build_layer",
        parent_layer = ":" + name + "__setup_layer",
        rule_type = "third_party_build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "bash",
            "-uec",
            "/build",
        ],
    )

    hoist(
        name = name,
        layer = ":" + name + "__build_layer",
        path = OUT_DIR.lstrip("/"),
        selector = [
            "-mindepth 1",
            "-maxdepth 1",
        ],
        force_dir = True,
        visibility = [
            "//antlir/...",
            "//metalos/...",
            "//third-party/...",
        ],
    )

def _new_script(build, install, prepare = ""):
    return script_t(
        prepare = prepare,
        build = build,
        install = install,
    )

def _library(name, *, include_path = "include", lib_path = "lib"):
    return dep_t(
        name = name,
        source = third_party_shim.library(name, name, "antlir"),
        paths = [include_path, lib_path],
    )

third_party = struct(
    # The native build target is the main mechanism of building third-party
    # packages directly from sources, inside an isolated btrfs layer.
    #
    # The source tarball is figured out by the third_party oss shim based on
    # the project name, unpacked along with dependencies in the layer and then
    # a script shape is used to provide the generic configure, make and install
    # operations (these are similar to gnu make concepts)
    # See //antlir/third-party subfolders for usage examples.
    native_build = _native_build,

    # This method constructs a build script to be used with native_build
    script = _new_script,

    # In order to specify build dependencies that were built with native_build,
    # the library call can be used. By default it will present the "include" and
    # "lib" folder from the target.
    # Note that native_build uses a mechanism to provide correct paths for
    # projects that use pkg-config, so it's usually just necessary to provide the
    # PKG_CONFIG_PATH and most prepare scripts should work.
    # See //antlir/third-party subfolders for usage examples.
    library = _library,
)
