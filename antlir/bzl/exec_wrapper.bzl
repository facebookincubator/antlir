# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "is_buck2")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")

def build_exec_wrapper(
        runnable,
        path_in_output = None,
        args = None,
        literal_preamble = "",
        shell_substitutable_preamble = ""):
    """Returns shell for a genrule that's intended to execute `runnable` with
    args and is compatible with buck2 repo-relative output paths.

    Important notes:

    - This returned shell should be interpolated into a genrule with kwargs
      `executable = True` and `cacheable = False`. The latter is critical as
      this rule contains on-disk paths.
    - As a result of the above, this genrule should NOT be embedded into other
      targets as those targets themselves may be cached. It should only be
      executed directly by users.
    """
    EOF = "EXEC_WRAPPER_EOF"
    for preamble in [literal_preamble, shell_substitutable_preamble]:
        if EOF in preamble.splitlines():
            fail("preamble {} had a '{}' line".format(preamble, EOF))

    # Note:  Notice we are using the `$(exe_target ...)` macro instead of just
    # plain old `$(exe ...)` when invoking the wrapped target binary. The
    # behavior difference is that buck will compile the resolved target path
    # against the `target platform` when using `$(exe_target ...)` vs using the
    # `host platform` when using `$(exe ...)`.  This matters here because the
    # execution environment for this wrapper will almost always be within a
    # runtime that matches the `target platform`.  A simple example is, if we
    # are using a `target platform` of `Fedora33`, which has a glibc version of
    # 2.32, we want the binary being invoked by this wrapper to be compiled
    # against glibc 2.32. It should be noted that inside Facebook, this doesn't
    # matter so much because there is generally no difference between the `host
    # platform` and the `target platform` due to how the runtimes are managed
    # and available as part of the aether. Also note: This feature of Buck is
    # pretty much undocumented since this is part of a yet to be described "new"
    # behavior.  There are test cases that cover this though:
    # https://github.com/facebook/buck/tree/master/test/com/facebook/buck/cli/configurations/testdata/exe_target
    return """
echo '#!/bin/sh' > "$TMP/out"
{maybe_repo_root_preamble}
cat >> "$TMP/out" << {EOF}
{shell_substitutable_preamble}
{EOF}
cat >> "$TMP/out" << '{EOF}'
{literal_preamble}
exec {maybe_repo_root_prefix}$(exe_target {runnable}){maybe_quoted_path_in_output} {args}
{EOF}
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
""".format(
        shell_substitutable_preamble = shell_substitutable_preamble,
        literal_preamble = literal_preamble,
        EOF = EOF,
        # The preamble is inserted as part of the genrule script itself as
        # opposed to the script the genule is creating.  This is used to
        # discover the repository root dynamically during build time to build a
        # full ABS path to the executable when using buck2. At first glance it
        # would appear that we should just be able to use `realpath` to find the
        # ABS path of the $(exe_target ..). That turned out to be fragile for
        # two reasons:
        #   - It relies on buck/buck2 to execute the construction of the genrule
        #     with a cwd of the repository root.  While this is currently the
        #     case as of this commit, it has changed multiple times in the
        #     recent past for buck2.  This approach makes the cwd irrelevant.
        #   - buck (v1) resolves certain binary types (looking at you python)
        #     via $(exe_target ...) as a full path + additional args, which
        #     means that the bash would have to parse it to get the path of the
        #     _actual_ buck runnable.  That is a nightmare.
        maybe_repo_root_preamble = """
binary_path=( $(exe {repo_root}))
repo_root=\\$( $binary_path )
echo "REPO_ROOT=$repo_root" >> "$TMP/out"
        """.format(
            repo_root = antlir_dep(":repo-root"),
        ) if is_buck2() else "",
        # The prefix is inserted into the generated script so that at runtime
        # the repository root which is discovered at build time is properly
        # expanded.
        maybe_repo_root_prefix = "$REPO_ROOT/" if is_buck2() else "",
        runnable = runnable,
        maybe_quoted_path_in_output = (
            "/" + shell.quote(path_in_output)
        ) if path_in_output else "",
        args = args if args else "",
    )
