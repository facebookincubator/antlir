# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":constants.bzl", "REPO_CFG")
load(":exec_wrapper.bzl", "build_exec_wrapper")
load(":oss_shim.bzl", "buck_genrule", "get_visibility")
load(":target_helpers.bzl", "wrap_target")

def _maybe_wrap_runtime_deps_as_build_time_deps(
        name,
        target,
        visibility,
        runs_in_build_steps_causes_slow_rebuilds,
        path_in_output = None):
    """
    If necessary (see "When..."), wraps `target` with a new target named
    `name`, in the current project.

    Returns `(False, target)` if unwrapped, or `(True, ':<name>')` otherwise.

    The build-time dependencies of the wrapper `:<name>` will include the
    run-time dependencies of `target`.

    Wrapping is commonly used when `image.layer` will run `target` as part
    of its build process, or when some target needs to be executable from
    inside an `image.layer`.

    IMPORTANT: The build artifact of `:<name>` is NOT cacheable, so if you
    include its contents in some other artifact, that artifact must ALSO
    become non-cacheable.

    ## Special situations

      - `path_in_output` sets the wrapper to execute a fixed file out of a
        directory that is output by an executable rule.

      - `runs_in_build_steps_causes_slow_rebuilds = True` allows the target
        being wrapped to be executed in an Antlir container as part of a
        Buck build step. This flag exists to speed up incremental rebuilds.

        Any image that installs an executable, which is later used in a
        build, *MUST* be invalidated whenever `$(exe <target>)` is
        invalidated. The `$(exe)` dependency is invalidated, roughly,
        whenever the source of the target changes. This makes sense --
        the code is doing something different, so we have to re-run it.

        However, for in-place images that include executable targets that
        NEVER run within an Antlir build step, this is invalidates too often.

        For example, if you install an in-place build of
            cxx_binary(name = "foo", srcs = ["foo.cpp"])
        into an image, the actual byte contents of the image will not be
        invalidated when `foo.cpp` is edited.  So, targets depending on the
        image, such as `image.*_unittest`s, do not have to be rebuilt.  This
        speeds up iterating on `buck test` or on `=container` targets.

    ## Why is wrapping needed?

    There are two reasons for wrapping.

      - The primary reason for this is that due to Buck limitations,
        `image.layer` cannot directly take on run-time dependencies (more on
        that below), so the wrapper makes ALL dependencies (run-time or
        build-time) look like build-time dependencies.

      - The second reason is to execute in-place (aka @mode/dev) binaries
        from inside an image -- in that case, the wrapper acts much like a
        symlink, although it ALSO has the effect of ensuring that the image
        gets rebuilt if any of the runtime dependencies of its contained
        executables change.  In many cases, this results in over-building in
        @mode/dev -- the more performant solution would be to have a tag on
        in-image executables signaling whether they are permitted to be used
        as part of the image build.  For most, the tag would say "no", and
        those would not need runtime dependency wrapping.  However, the
        extra complexity makes this idea "far future".

    Here is what would go wrong if we just passed `target` directly to
    `image.layer` to execute:

     - For concreteness' sake, let's say that `target` needs to be
       executed by the `image.layer` build script.

     - `image.layer` will use $(query_targets_and_outputs) to find the
       output path for `target`.

     - Suppose that `target`'s source code CHANGED since the last time our
       layer was built.

     - Furthermore, suppose that the output of `target` is a thin wrapper,
       such as what happens with in-place Python executables in @mode/dev.
       Even though the FUNCTIONALITY of the Python executable has changed,
       the actual build output will remain the same.

     - At this point, the output path that's included in the bash command of
       the layer's genrule has NOT changed.  The file referred to by that
       output path has NOT changed.  Only its run-time dependencies (the
       in-place symlinks to the actual `.py` files) have changed.
       Therefore, as far as build-time dependencies of the layer are
       concerned, the layer does not need to re-build: the inputs of the
       layer genrule are bitwise the same as the inputs before any changes
       to `target`'s source code.

       In other words, although `target` itself WOULD get rebuilt due to
       source code changes, the layer that depends on that target WOULD NOT
       get rebuilt, because it does not consider the `.py` files inside the
       in-place Python link-tree to be build-time inputs.  Those are runtime
       dependencies.  Peruse the docs here for a Buck perspective:
           https://github.com/facebook/buck/blob/master/src/com/facebook/
           buck/core/rules/attr/HasRuntimeDeps.java

    We could avoid the wrapper if we could add `target` as a **runtime
    dependency** to the `image.layer` genrule.  However, Buck does not make
    this possible.  It is possible to add runtime dependencies on targets
    that are KNOWN to the `image.layer` macro at parse time, since one could
    then use `$(exe)` -- which says "rebuild me if the mentioned target's
    runtime dependencies have changed".  But because we want to support
    composition of layers via features, `$(exe)` does not help -- the layer
    has to discover its features' dependencies via a query.  Unfortunately,
    Buck's query facilities of today only allow making build-time
    dependencies (not runtime dependencies).  So supporting the right API
    would require a change in Buck.  Either of these would do:

      - Support adding query-determined runtime dependencies to
        genrules -- via a special-purpose macro, a macro modifier, or a rule
        attribute.

      - Support Bazel-style providers, which would let the layer
        implementation directly access the data collated by its features.
        Then, the layer could just issue $(exe) macros for all runtime-
        dependency targets.  NB: This would bring a build speed win, too.

    ## When should we NOT wrap?

    This build-time -> run-time dependency wrapper doesn't work inside
    @mode/opt containers, since those (deliberately) don't bind-mount the
    repo inside.  They are supposed to be self-contained and ready for
    production.

    However, in @mode/opt we don't care about the build-time / run-time
    dependency problem since C++ & Python build artifacts are
    self-contained, making the two dependency types identical.

    Note: Because our CI always lists targets with @mode/dev, projects that
    can only build in @mode/opt will fail because it will not be able to find
    this target. Output a dummy target (in place of the wrapper) in this case
    to appease it.
    """
    if not REPO_CFG.artifacts_require_repo:
        buck_genrule(
            name = name,
            bash = 'touch "$OUT"',
            antlir_rule = "user-internal",
        )
        return False, target

    if runs_in_build_steps_causes_slow_rebuilds:
        literal_preamble = ""
        unquoted_heredoc_preamble = (
            "# New output each build: \\$(date) $$ $PID $RANDOM $RANDOM"
        )
    else:
        # NB: The env var marking is now redundant with the more robust
        # NIS domainname marking, so we could just rip it out. However,
        # it should be cheaper NOT to run a subprocess, so keep it for now.
        #
        # IMPORTANT: Avoid bashisms, this can run under busybox's `ash`.
        literal_preamble = (
            """\
if [ "$ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP" != "1" ] &&
   [ "`"$REPO_ROOT"/$(location {nis_domain})`" != "AntlirNotABuildStep" ] ; then
    cat >&2 << 'EOF'
AntlirUserError: Ran Buck target `{target}` from an Antlir build step, """ +
            """\
but it was installed without `runs_in_build_steps_causes_slow_rebuilds = True`.
EOF
    exit 1
fi
"""
        ).format(
            # No attempt at shell-quoting since the Buck target charset
            # doesn't need quoting in heredocs.
            target = target,
            nis_domain = "fbcode//antlir/nspawn_in_subvol/nisdomain:nis_domainname",
        )
        unquoted_heredoc_preamble = ""

    buck_genrule(
        name = name,
        bash = build_exec_wrapper(
            runnable = target,
            path_in_output = path_in_output,
            literal_preamble = literal_preamble,
            unquoted_heredoc_preamble = unquoted_heredoc_preamble,
        ),
        # We deliberately generate a unique output on each rebuild.
        cacheable = False,
        # Whatever we wrap was executable, so the wrapper might as well be, too
        executable = True,
        visibility = get_visibility(visibility),
        antlir_rule = "user-internal",
    )

    return True, ":" + name

def maybe_wrap_executable_target(target, wrap_suffix, **kwargs):
    """
    Docs on `_maybe_wrap_runtime_deps_as_build_time_deps'.  This variant
    automatically names the wrapped target, and reuses an existing one.
    """
    exists, wrapped_target = wrap_target(target, wrap_suffix)

    # Reuse a pre-existing wrapper for the same target -- with our 120-bit
    # secure hashes, collisions are practically impossible.
    if exists:
        # With self-contained artifacts, we create a dummy wrapper target to
        # satisfy the CI target determinator, but we must not use it.
        if not REPO_CFG.artifacts_require_repo:
            return False, target  # Don't create another dummy wrapper
        return True, ":" + wrapped_target

    # The `wrap_runtime_deps_as_build_time_deps` docblock explains this:
    was_wrapped, maybe_target = _maybe_wrap_runtime_deps_as_build_time_deps(
        name = wrapped_target,
        target = target,
        **kwargs
    )
    return was_wrapped, maybe_target
