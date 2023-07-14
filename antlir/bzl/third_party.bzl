# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":build_defs.bzl", "buck_genrule", third_party_shim = "third_party")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":image.bzl", "image")
load(":third_party.shape.bzl", "dep_t", "script_t")

PREFIX = "/third-party-build"
SRC_TGZ = paths.join(PREFIX, "source.tar.gz")
PATCHES_DIR = paths.join(PREFIX, "patches")
SRC_DIR = paths.join(PREFIX, "src")
DEPS_DIR = paths.join(PREFIX, "deps")
OUTPUT_DIR = "/output"

def _build(name, features, script, src, deps = None, **kwargs):
    deps = deps or []

    OUTPUT_DIR = paths.join(DEPS_DIR, name)

    buck_genrule(
        name = name + "__build_script",
        out = "out",
        cmd = ("""
cat > $TMP/out << 'EOF'
#!/bin/bash

set -ue
set -o pipefail

# unpack the source in build dir
cd "{src_dir}"
tar xzf {src} --strip-components=1

# Patch sources
for p in \\$(ls -A {patches_dir}); do
    patch < {patches_dir}/$p;
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
        antlir_rule = "user-internal",
    )

    image.layer(
        name = name + "__setup_layer",
        parent_layer = flavor_helpers.get_build_appliance(),
        features = features + [
            feature.ensure_dirs_exist(DEPS_DIR),
            feature.ensure_dirs_exist(OUTPUT_DIR),
            feature.ensure_dirs_exist(PATCHES_DIR),
            feature.ensure_dirs_exist(SRC_DIR),
            feature.install(
                src,
                SRC_TGZ,
            ),
            feature.install(
                ":" + name + "__build_script",
                "/build.sh",
                mode = "a+x",
            ),
            feature.rpms_install([
                "tar",
            ]),
        ] + [
            feature.layer_mount(
                dep.source,
                paths.join(DEPS_DIR, dep.name),
            )
            for dep in deps
        ] + ([
            feature.install(i, paths.join(PATCHES_DIR, i.split(":")[1]))
            for i in script.patches
        ] if script.patches else []),
        flavor = flavor_helpers.get_flavor_from_build_appliance(
            flavor_helpers.get_build_appliance(),
        ),
    )

    image.genrule_layer(
        name = name + "__build_layer",
        parent_layer = ":" + name + "__setup_layer",
        rule_type = "third_party_build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "/build.sh",
        ],
        visibility = ["//antlir/..."],
    )

    image.layer(
        name = name,
        features = [
            feature.clone(
                ":" + name + "__build_layer",
                OUTPUT_DIR + "/",
                "/",
            ),
        ],
        flavor = flavor_helpers.get_antlir_linux_flavor(),
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
