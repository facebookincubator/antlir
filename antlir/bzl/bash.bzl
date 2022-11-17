# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "normalize_target")

def wrap_bash_build_in_common_boilerplate(
        bash,
        target_name,
        deps_query = None):
    """
    Wrap a bash script to run in an environment that takes care of
    buck-image-out existence, permissions and logging.

    If the script has some dependencies that can't adequately be tracked by buck
    with $(location) (for example, a `python_binary` in @//mode/dev),
    `deps_query` can be used to insert a cache-buster into the script so that a
    `buck_genrule` containing this script contents will re-run when any of the
    targets matching `deps_query` are changed.
    """
    args = wrap_bash_build_in_common_boilerplate_args(bash, target_name, deps_query)
    cmd, args = args[0], args[1:]
    return cmd + " " + " ".join([_maybe_quote(a) for a in args])

def _maybe_quote(arg):
    # buck1 and 2 have weird and diverging behavior with quoting around
    # $(query_outputs) so just expect that the arg here will be properly shell
    # quoted even after buck expansion (caller has to be careful that this is
    # true, but it's just a matter of wrapping in single quotes)
    if arg.startswith("BUCK_QUERY:"):
        return arg
    if arg in ("$OUT", "$TMP"):
        return arg
    return shell.quote(arg)

def wrap_bash_build_in_common_boilerplate_args(
        bash,
        target_name,
        deps_query = None):
    args = [
        "$(exe {})".format(antlir_dep("builder:builder-with-defaults")),
        "--tmp-dir",
        "$TMP",
        "--out",
        "$OUT",
        "--label={}".format(normalize_target(":" + target_name)),
    ]
    if deps_query:
        args += ["--cache-buster", "BUCK_QUERY:'$(query_outputs '{}')'".format(deps_query)]
    args += ["generic", "--", "/bin/bash", "-e", "-c", bash]
    return args
