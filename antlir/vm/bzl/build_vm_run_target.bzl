# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "is_buck2")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":types.bzl", "api")

# Note: This should be merged with the genrule in `wrap_runtime_deps.bzl`, they are
# almost exactly the same except for the `dynamic_path_in_output` part of the later.
# This wasn't refactored out initially simply to avoid a large number of changes
# at once.
def _build_exec_wrapper(runnable, path_in_output = None, args = None):
    body = """
echo "#!/bin/sh" > "$TMP/out"
{maybe_repo_root_preamble}
cat >> "$TMP/out" << 'EOF'
exec {maybe_repo_root_prefix}$(exe {runnable}){maybe_quoted_path_in_output}{args}
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
    """.format(
        maybe_repo_root_preamble = """
            binary_path=( $(exe {repo_root}))
            repo_root=\\$( $binary_path )
            echo "REPO_ROOT=$repo_root" >> "$TMP/out"
        """.format(
            repo_root = antlir_dep(":repo-root"),
        ) if is_buck2() else "",
        # The prefix is inserted into the generated script so that at
        # runtime the repository root which is discovered at build time is
        # properly expanded.
        maybe_repo_root_prefix = "$REPO_ROOT/" if is_buck2() else "",
        runnable = runnable,
        maybe_quoted_path_in_output = (
            "/" + shell.quote(path_in_output)
        ) if path_in_output else "",
        args = args if args else "",
    )
    return body

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
        bash = _build_exec_wrapper(
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
