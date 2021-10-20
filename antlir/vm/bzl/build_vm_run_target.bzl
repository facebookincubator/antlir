# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
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
        exe_target = "//antlir/vm:run"):
    vm_opts = vm_opts or api.opts.new()
    buck_genrule(
        name = name,
        antlir_rule = "user-internal",
        bash = """
cat > "$TMP/out" << 'EOF'
#!/bin/sh
set -ue -o pipefail -o noclobber
exec $(exe {exe_target}) \
--opts {opts_quoted} \
{extra_args} \
"$@"
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
        """.format(
            exe_target = exe_target,
            extra_args = " ".join(args) if args else "",
            opts_quoted = shell.quote(shape.do_not_cache_me_json(vm_opts)),
        ),
        cacheable = False,
        executable = True,
        visibility = [],
    )

    return name
