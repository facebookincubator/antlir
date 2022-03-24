# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:exec_wrapper.bzl", "build_exec_wrapper")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":types.bzl", "api")

def build_vm_run_target(
        # The name of the runnable target
        name,
        # An instance of a vm_opts_t shape.
        vm_opts = None,
        # A list of additional cli args to pass to the provided exe_target.
        # This is passed directly to the `exe_target` so they should already be
        # properly formatted.
        args = None,
        # The exe target to execute.
        exe_target = antlir_dep("vm:run")):
    vm_opts = vm_opts or api.opts.new()
    buck_genrule(
        name = name,
        antlir_rule = "user-internal",
        bash = build_exec_wrapper(
            runnable = exe_target,
            args = """ --opts {opts_quoted} {extra_args} "$@"
            """.format(
                extra_args = " ".join(args) if args else "",
                opts_quoted = shell.quote(shape.do_not_cache_me_json(vm_opts)),
            ),
        ),
        cacheable = False,
        executable = True,
        visibility = [],
    )

    return name
