# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load(":build_defs.bzl", "buck_genrule", "internal_external", third_party_shim = "third_party")
load(":third_party.shape.bzl", "dep_t", "script_t")

PREFIX = "/third-party-build"
SRC_TGZ = paths.join(PREFIX, "source.tar.gz")
PATCHES_DIR = paths.join(PREFIX, "patches")
SRC_DIR = paths.join(PREFIX, "src")
DEPS_DIR = paths.join(PREFIX, "deps")
OUTPUT_DIR = "/output"

def _build(name, features, script, src, deps = None, dnf_additional_repos = None, **kwargs):
    deps = deps or []
    OUTPUT_DIR = paths.join(DEPS_DIR, name)

    buck_genrule(
        name = name + "__build_script",
        out = "out",
        cmd = ("""
cat > $TMP/out << 'EOF'
#!/bin/bash

set -uex
set -o pipefail

# unpack the source in build dir
cd "{src_dir}"
tar xzf {src} --strip-components=1

# Patch sources
for p in \\$(ls -A {patches_dir}); do
    test -f {patches_dir}/$p;
    patch --strip=0 --verbose < {patches_dir}/$p;
done

export OUTPUT="{output_dir}/"
export PKG_CONFIG_PATH="\\$(find {deps_dir} -type d -name pkgconfig | paste -sd ':')"
export MAKEFLAGS=-j

{prepare}
{build}
{install}
EOF
mv $TMP/out $OUT
chmod +x $OUT
        """).format(
            src = SRC_TGZ,
            prepare = script.prepare if script.prepare else "",
            build = script.build,
            install = script.install,
            deps_dir = DEPS_DIR,
            patches_dir = PATCHES_DIR,
            src_dir = SRC_DIR,
            output_dir = OUTPUT_DIR,
        ),
    )

    image.layer(
        name = name + "__setup_layer",
        parent_layer = internal_external(
            fb = "antlir//antlir/third-party:build-base",
            oss = "//third-party/antlir:build-base",
        ),
        dnf_additional_repos = dnf_additional_repos,
        features = features + [
            feature.ensure_dirs_exist(dirs = DEPS_DIR),
            feature.ensure_dirs_exist(dirs = OUTPUT_DIR),
            feature.ensure_dirs_exist(dirs = PATCHES_DIR),
            feature.ensure_dirs_exist(dirs = SRC_DIR),
            feature.install(
                src = src,
                dst = SRC_TGZ,
            ),
            feature.install(
                src = ":" + name + "__build_script",
                dst = "/build.sh",
                mode = "a+x",
            ),
            feature.rpms_install(rpms = [
                "tar",
            ]),
        ] + [
            [
                feature.ensure_dirs_exist(
                    dirs = paths.join(DEPS_DIR, dep.name),
                ),
                # TODO(T174899613) use feature.layer_mount if/when it gets mounted
                # in the feature.genrule environment
                feature.clone(
                    src_layer = dep.source,
                    src_path = "/",
                    dst_path = paths.join(DEPS_DIR, dep.name) + "/",
                ),
            ]
            for dep in deps
        ] + ([
            feature.install(src = i, dst = paths.join(PATCHES_DIR, i.split(":")[1]))
            for i in script.patches
        ] if script.patches else []),
    )

    image.layer(
        name = name + "__build_layer",
        parent_layer = ":" + name + "__setup_layer",
        visibility = ["//antlir/..."],
        features = [feature.genrule(
            user = "root",
            cmd = [
                "/build.sh",
            ],
        )],
    )

    image.layer(
        name = name,
        features = [
            feature.clone(
                src_layer = ":" + name + "__build_layer",
                src_path = OUTPUT_DIR + "/",
                dst_path = "/",
            ),
        ],
        **kwargs
    )

def _new_script(**kwargs):
    return script_t(**kwargs)

def _library(name, *, lib_path = "lib"):
    return dep_t(
        name = name,
        source = third_party_shim.library(name, name, "antlir"),
        paths = [
            lib_path,
        ],
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
    build = _build,

    # This method constructs a build script to be used with build
    script = _new_script,

    # In order to specify build dependencies that were built with build,
    # the library call can be used. By default it will present the "include" and
    # "lib" folder from the target.
    # Note that build uses a mechanism to provide correct paths for
    # projects that use pkg-config, so it's usually just necessary to provide the
    # PKG_CONFIG_PATH and most prepare scripts should work.
    # See //antlir/third-party subfolders for usage examples.
    library = _library,

    # Convienence to provide a path to the third-party source tree via the existing
    # shim. This makes this `third_party.bzl` API the entry point.
    source = third_party_shim.source,
)
