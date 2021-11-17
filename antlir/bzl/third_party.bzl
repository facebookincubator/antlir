# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":constants.bzl", "REPO_CFG")
load(":image.bzl", "image")
load(":oss_shim.bzl", "buck_genrule", third_party_shim = "third_party")
load(":shape.bzl", "shape")

PREFIX = "/build"
SRC_TGZ = PREFIX + "/source.tar.gz"
SRC_DIR = PREFIX + "/src"
DEPS_DIR = PREFIX + "/deps"
STAGE_DIR = PREFIX + "/stage"

def _hoist(name, out, layer, path, **buck_genrule_kwargs):
    """Creates a rule to lift an artifact out of the image it was built in."""
    buck_genrule(
        name = name,
        out = out,
        bash = '''
            binary_path=( $(exe //antlir:find-built-subvol) )
            layer_loc="$(location {layer})"
            sv_path=\\$( "${{binary_path[@]}}" "$layer_loc" )
            cp -r "$sv_path{path}" --no-clobber "$OUT"
        '''.format(
            layer = ":" + layer,
            path = path,
        ),
        **buck_genrule_kwargs
    )

def _cmd_for_dependency_setup(dependency):
    return " && ".join([
        "cp -r {stage}/{name}/{path} {deps}".format(
            stage = STAGE_DIR,
            name = dependency.name,
            path = path,
            deps = DEPS_DIR,
        )
        for path in dependency.paths
    ])

def _build(base_features, bash, *, version = "latest", name = "build", dependencies = [], project = None):
    if not project:
        project = paths.basename(package_name())
    source_target = third_party_shim.source(project)

    image.layer(
        name = "base-layer",
        features = [
            image.ensure_dirs_exist(SRC_DIR),
            image.ensure_dirs_exist(DEPS_DIR),
            image.ensure_dirs_exist(STAGE_DIR),
            feature.install(source_target, SRC_TGZ),
            image.rpms_install(["tar"]),
        ] + base_features,
        flavor = REPO_CFG.antlir_linux_flavor,
    )

    image.layer(
        name = "deps-layer",
        parent_layer = ":base-layer",
        features = [
            feature.install(dep.source, "{}/{}".format(STAGE_DIR, dep.name))
            for dep in dependencies
        ],
    )

    image.genrule_layer(
        name = "unpack-layer",
        parent_layer = ":deps-layer",
        rule_type = "third_party_build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "tar",
            "xzf",
            SRC_TGZ,
            "--strip-components=1",
            "--directory=" + SRC_DIR,
        ],
    )

    image.genrule_layer(
        name = "deps-unpack-layer",
        parent_layer = ":unpack-layer",
        rule_type = "third_party_build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "bash",
            "-uec",
            " && ".join([
                _cmd_for_dependency_setup(dep)
                for dep in dependencies
            ]),
        ],
    )

    image.genrule_layer(
        name = "build-layer",
        parent_layer = ":deps-unpack-layer",
        rule_type = "third_party_build",
        antlir_rule = "user-internal",
        user = "root",
        cmd = [
            "bash",
            "-uec",
            """
            cd {src_dir}

            export DEPS={deps_dir}
            export STAGE={stage_dir}
            {script}
            """.format(
                project = project,
                version = version,
                src_dir = SRC_DIR,
                deps_dir = DEPS_DIR,
                stage_dir = STAGE_DIR,
                script = bash,
            ),
        ],
    )

    _hoist(
        name = name,
        out = "out",
        layer = "build-layer",
        path = STAGE_DIR,
    )

_dep_t = shape.shape(
    name = str,
    source = shape.target(),
    paths = shape.list(str),
)

def _library(name, *, include_path = "include", lib_path = "lib"):
    return shape.new(
        _dep_t,
        name = name,
        source = third_party_shim.library(name, "build", "antlir"),
        paths = [include_path, lib_path],
    )

def _oss_build(*, project = None, name = "build"):
    if not project:
        project = paths.basename(package_name())

    buck_genrule(
        name = "build",
        out = "out",
        bash = """
            cp --reflink=auto -r $(location //antlir/third-party/{project}:{name}) "$OUT"
        """.format(
            project = project,
            name = name,
        ),
    )

third_party = struct(
    build = _build,
    library = _library,
    oss_build = _oss_build,
)
