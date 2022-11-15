# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:build_defs.bzl", "buck_genrule", "is_buck2")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "normalize_target")
load(":bash.2.bzl?v2_only", buck2_boilerplate_genrule = "boilerplate_genrule")

def _buck1_boilerplate_genrule(
        name,
        bash,
        out = "out",
        deps_query = None,
        antlir_rule = None,
        **genrule_kwargs):
    buck_genrule(
        name = name,
        bash = _make_shell_script(bash, name, deps_query),
        out = out,
        antlir_rule = antlir_rule,
        **genrule_kwargs
    )

"""
Wrap a bash script to run in an environment that takes care of
buck-image-out existence, permissions and logging.

If the script has some dependencies that can't adequately be tracked by buck
with $(location) (for example, a `python_binary` in @//mode/dev),
`deps_query` can be used to insert a cache-buster into the script so that a
`buck_genrule` containing this script contents will re-run when any of the
targets matching `deps_query` are changed.
"""
boilerplate_genrule = buck2_boilerplate_genrule if is_buck2() else _buck1_boilerplate_genrule

def _make_shell_script(
        bash,
        target_name,
        deps_query = None):
    cmd = "$(exe {})".format(antlir_dep("builder:builder-with-defaults"))
    args = [
        "--tmp-dir",
        "$TMP",
        "--out",
        "$OUT",
        "--label={}".format(normalize_target(":" + target_name)),
    ]
    if deps_query:
        args += ["--cache-buster", "BUCK_QUERY:'$(query_outputs '{}')'".format(deps_query)]
    args += ["--", "/bin/bash", "-e", "-c", bash]
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
