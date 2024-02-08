# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load(":build_defs.bzl", "buck_command_alias")
load(":query.bzl", "layer_deps_query")
load(":target_helpers.bzl", "antlir_dep", "normalize_target", "targets_and_outputs_arg_list")

def container_target_name(name):
    return name + "=container"

def systemd_target_name(name):
    return name + "=systemd"

def _add_run_in_subvol_target(name, kind, extra_args = None):
    target = name + "=" + kind
    buck_command_alias(
        name = target,
        exe = antlir_dep("nspawn_in_subvol:run"),
        args = [
            "--setenv=ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1",
            "--container-not-part-of-build-step",
            "$(location {})".format(antlir_dep(
                "nspawn_in_subvol/nisdomain:nis_domainname",
            )),
            "--layer",
            "$(location {})".format(shell.quote(":" + name)),
        ] + (extra_args or []) + targets_and_outputs_arg_list(
            name = target,
            query = layer_deps_query(layer = normalize_target(":" + name)),
        ),
    )

def add_runtime_targets(layer, runtime):
    runtime = runtime or []
    if "container" not in runtime:
        runtime.append("container")

    for elem in runtime:
        if elem == "container":
            _add_run_in_subvol_target(layer, "container")
        elif elem == "systemd":
            _add_run_in_subvol_target(layer, "systemd", extra_args = ["--boot", "--append-console"])
        else:
            fail("Unsupported runtime encountered: {}".format(elem))
