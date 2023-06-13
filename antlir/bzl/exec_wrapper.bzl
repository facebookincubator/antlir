# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:build_defs.bzl", "antlir_buck_env", "is_buck2")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")

def build_exec_wrapper(
        runnable,
        path_in_output = None,
        # Keep these compliant with POSIX `sh` -- MetalOS `initrd` lacks `bash`
        raw_shell_args = '"$@"',
        literal_preamble = "",
        unquoted_heredoc_preamble = ""):
    """
    Returns shell for a genrule that's intended to execute `runnable` with
    the same args as supplied (default) or modified ones.  Unlike a naive
    implementation, this supports Buck2 repo-relative output paths.

    Important notes:

    - This returned shell should be interpolated into a genrule with kwargs
      `executable = True` and `cacheable = False`. The latter is critical as
      this rule contains on-disk paths.
    - As a result of the above, this genrule should NOT be embedded into other
      targets as those targets themselves may be cached. It should only be
      executed directly by users.
    - Stick to POSIX `sh` in `raw_shell_args` & preambles.
    """
    EOF = "EXEC_WRAPPER_EOF"
    for preamble in [literal_preamble, unquoted_heredoc_preamble]:
        if EOF in preamble.splitlines():
            fail("`preamble={}` had a '{}' line".format(repr(preamble), EOF))

    # In Buck1: `$()` macro paths are absolute.  `$REPO_ROOT` is empty
    # so that `$REPO_ROOT/abs/path` expands to `/abs/path`.
    #
    # In Buck2: `$()` macro paths are repo-relative.  We compute the
    # absolute path of `$REPO_ROOT` at build-time (in this genrule), and
    # store literal value into the wrapper script.  When the wrapper
    # runs, `$REPO_ROOT/rel/path` expands to `/repo/root/rel/path`.
    #
    # Rationale: At first glance it would appear that we should just be
    # able to use `realpath` to find the absolute path of the
    # $(exe_target ..).  That turned out to be fragile for two reasons:
    #   - It relies on buck/buck2 to execute the construction of the genrule
    #     with a cwd of the repository root.  While this is currently the
    #     case as of this commit, it has changed multiple times in the
    #     recent past for buck2.  This approach makes the cwd irrelevant.
    #   - buck (v1) resolves certain binary types (looking at you python)
    #     via $(exe_target ...) as a full path + additional args, which
    #     means that the bash would have to parse it to get the path of the
    #     _actual_ buck runnable.  That is a nightmare.
    if is_buck2():
        add_repo_root_to_wrapper = """\
binary_path=( $(exe {repo_root}))
repo_root=\\$( $binary_path )
echo "REPO_ROOT=$repo_root" >> "$TMP/out"
        """.format(repo_root = antlir_dep(":repo-root"))
    else:
        add_repo_root_to_wrapper = '''\
echo "REPO_ROOT=" >> "$TMP/out"
'''

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
    #
    # Pro-tip: If you ever find yourself debugging a wrapped systemd generator,
    # add `-x` to the hashbang, and follow it with:
    #   : 2>/dev/kmsg && exec 2> /dev/kmsg
    #   : 1>/dev/kmsg && exec 1> /dev/kmsg
    return """
cat << '{EOF}' > "$TMP/out"
#!/bin/sh
{EOF}
{add_repo_root_to_wrapper}
cat >> "$TMP/out" << {EOF}
{unquoted_heredoc_preamble}
{EOF}
cat >> "$TMP/out" << '{EOF}'
export ANTLIR_BUCK="{antlir_buck}"
{literal_preamble}
if [[ "$INSIDE_RE_WORKER" == "1" ]]; then
    export REPO_ROOT="/re_cwd"
fi
exec "$REPO_ROOT/"$(exe_target {runnable}){maybe_quoted_path_in_output} {args}
{EOF}
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
""".format(
        add_repo_root_to_wrapper = add_repo_root_to_wrapper,
        EOF = EOF,
        unquoted_heredoc_preamble = unquoted_heredoc_preamble,
        literal_preamble = literal_preamble,
        runnable = runnable,
        maybe_quoted_path_in_output = (
            "/" + shell.quote(path_in_output)
        ) if path_in_output else "",
        args = raw_shell_args,
        antlir_buck = antlir_buck_env(),
    )
