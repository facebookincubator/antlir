# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":image.bzl", "image")
load(":oss_shim.bzl", "buck_genrule", third_party_shim = "third_party")
load(":third_party.shape.bzl", "dep_t", "script_t")

PREFIX = "/third-party-build"
SRC_TGZ = paths.join(PREFIX, "source.tar.gz")
SRC_DIR = paths.join(PREFIX, "src")
DEPS_DIR = paths.join(PREFIX, "deps")
OUTPUT_DIR = "/output"

def _cmd_prepare_dependency(dependency):
    """ Provide the .pc file for the dep in the right place. """
    return "\n".join([
        "cp -a {deps}/{name}/{path}/pkgconfig/*.pc {deps}/pkgconfig/".format(
            deps = DEPS_DIR,
            name = dependency.name,
            path = path,
        )
        for path in dependency.paths
    ])

def _build(name, features, script, src, deps = None, **kwargs):
    deps = deps or []

    prepare_deps = "\n".join([
        _cmd_prepare_dependency(dep)
        for dep in deps
    ])

    OUTPUT_DIR = paths.join(DEPS_DIR, name)

    buck_genrule(
        name = name + "__build_script",
        out = "out",
        cmd = ("""
cat > $TMP/out << 'EOF'
#!/bin/bash

set -ue
set -o pipefail

# copy all specified dependencies
mkdir -p "{deps_dir}/pkgconfig"
{prepare_deps}

# unpack the source in build dir
cd "{src_dir}"
tar xzf {src} --strip-components=1

export OUTPUT="{output_dir}/"
export PKG_CONFIG_PATH="{deps_dir}/pkgconfig"
export MAKEFLAGS=-j

{prepare}
{build}
{install}
EOF
mv $TMP/out $OUT
chmod +x $OUT
        """).format(
            src = SRC_TGZ,
            prepare_deps = prepare_deps,
            prepare = script.prepare,
            build = script.build,
            install = script.install,
            deps_dir = DEPS_DIR,
            src_dir = SRC_DIR,
            output_dir = OUTPUT_DIR,
        ),
    )

    image.layer(
        name = name + "__setup_layer",
        parent_layer = flavor_helpers.get_build_appliance(),
        features = [
            feature.ensure_dirs_exist(DEPS_DIR),
            feature.ensure_dirs_exist(OUTPUT_DIR),
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
        ] + features + [
            feature.layer_mount(
                dep.source,
                paths.join(DEPS_DIR, dep.name),
            )
            for dep in deps
        ],
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

def _new_script(build, install, prepare = ""):
    return script_t(
        prepare = prepare,
        build = build,
        install = install,
    )

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
