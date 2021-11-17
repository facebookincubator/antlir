# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":constants.bzl", "REPO_CFG")
load(":image.bzl", "image")
load(":oss_shim.bzl", "buck_genrule", third_party_shim = "third_party")

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

def _build(base_features, bash, *, version = "latest", name = "build", dependencies = [], project = None):
    PREFIX = "/build"
    SRC_DIR = PREFIX + "/src"
    DEPS_DIR = PREFIX + "/deps"
    STAGE_DIR = PREFIX + "/stage"

    if not project:
        project = paths.basename(package_name())
    source_target = third_party_shim.source(project)

    image.layer(
        name = "base-layer",
        features = [
            image.ensure_dirs_exist(SRC_DIR),
            image.ensure_dirs_exist(DEPS_DIR),
            image.ensure_dirs_exist(STAGE_DIR),
            feature.install(source_target, "source.tar.gz"),
            image.rpms_install(["tar"]),
        ] + base_features,
        flavor = REPO_CFG.antlir_linux_flavor,
    )

    image.layer(
        name = "deps-layer",
        parent_layer = ":base-layer",
        features = [
            feature.install(target, "{}/{}".format(DEPS_DIR, dep))
            for target, dep in dependencies
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
            "/source.tar.gz",
            "--strip-components=1",
            "--directory=" + SRC_DIR,
        ],
    )

    image.genrule_layer(
        name = "build-layer",
        parent_layer = ":unpack-layer",
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
        out = ".",
        layer = "build-layer",
        path = STAGE_DIR,
    )

third_party = struct(
    build = _build,
)
