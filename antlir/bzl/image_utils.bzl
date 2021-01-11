# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":oss_shim.bzl", "target_utils")

def _wrap_bash_build_in_common_boilerplate(
        self_dependency,
        bash,
        rule_type,
        target_name,
        volume_min_free_bytes = None):
    return """
    # CAREFUL: To avoid inadvertently masking errors, we should
    # only perform command substitutions with variable
    # assignments.
    set -ue -o pipefail

    # Ensures that changes to the sources of the rule macros cause automatic
    # builds & tests on the artifacts they produce.
    echo $(location {self_dependency}) > /dev/null

    start_time=\\$(date +%s)
    # Common sense would tell us to find helper programs via:
    #   os.path.dirname(os.path.abspath(__file__))
    # The benefit of using \\$(exe) is that it does not bake an absolute
    # paths into our command.  This means the Buck cache continues working
    # even if the user moves the repo.  `exe` vs `location` is explained in
    # `image_package.bzl`.  We need `binary_path` because the `exe` macro
    # won't get expanded inside a \\$( ...  ) context.
    binary_path=( $(exe //antlir:artifacts-dir) )
    artifacts_dir=\\$( "${{binary_path[@]}}" )

    # Future-proofing: keep all Buck target subvolumes under
    # "targets/" in the per-repo volume, so that we can easily
    # add other types of subvolumes in the future.
    binary_path=( $(exe //antlir:volume-for-repo) )
    volume_dir=\\$( "${{binary_path[@]}}" "$artifacts_dir" {min_free_bytes} )
    subvolumes_dir="$volume_dir/targets"
    mkdir -m 0700 -p "$subvolumes_dir"

    # Capture output to a tempfile to hide logspam on successful runs.
    my_log=`mktemp`

    log_on_error() {{
      exit_code="$?"
      # Always persist the log for debugging purposes.
      collected_logs="$artifacts_dir/image_build.log"
      (
          echo "\n\\$(date) --" \
            "\\$(($(date +%s) - start_time)) sec --" \
            "{log_description}\n"
          cat "$my_log" || :
      ) |& flock "$collected_logs" tee -a "$collected_logs"
      # If we had an error, also dump the log to stderr.
      if [[ "$exit_code" != 0 || -n "${{ANTLIR_DEBUG:-}}" ]] ; then
        cat "$my_log" 1>&2
      fi
      rm "$my_log"
    }}
    # Careful: do NOT replace this with (...) || (...), it will lead
    # to `set -e` not working as you expect, because bash is awful.
    trap log_on_error EXIT

    (
      # Log all commands now that stderr is redirected.
      set -x

      {bash}

      # It is always a terrible idea to mutate Buck outputs after creation.
      # We have two special reasons that make it even more terrible:
      #  - [image_layer] Uses a hardlink-based refcounting scheme, as
      #    and keeps subvolumes in a special location.
      #  - [image_package] Speeds up the build for the `sendstream_stack`
      #    format by hardlinking duplicated outputs between targets.
      #
      # Not using "chmod -R" since Buck cleanup is fragile and cannot handle
      # read-only directories.
      find "$OUT" '!' -type d -print0 | xargs -0 --no-run-if-empty chmod a-w
    ) &> >(
      # We should not write directly to the file because it appears that
      # `systemd-nspawn` with a non-root user will `chown` its stderr
      # or stdout. This seems insane, and it prevents us from accessing
      # our own logs here. So, proxy the log through a dummy `cat` to
      # prevent that `chown` from being able to find the underlying file.
      cat > "$my_log"
    )
    """.format(
        bash = bash,
        min_free_bytes = volume_min_free_bytes if volume_min_free_bytes else "None",
        log_description = "{}:{}(name={})".format(
            native.package_name(),
            rule_type,
            target_name,
        ),
        self_dependency = self_dependency,
    )

def _current_target(target_name):
    return target_utils.to_label(
        # Note: we don't use the `config.get_current_repo_name()` here because
        # currently in the OSS setup the current repo ends up being `@`, which
        # doesn't work when we compile the layer. It doesn't work because
        # a target like `//antlir/compiler/test_images:parent_layer` is not
        # equivalent to `@//antlir/compiler/test_images:parent_layer`.
        # Technically we should use the current repo name when constructing
        # __all__ target labels. That would require a hefty refactor for all
        # usages of hard coded targets. This should be done but can wait until
        # the OSS repository is ready to be embedded/included in other projects
        # as a proper repo/cell.
        "",
        native.package_name(),
        target_name,
    )

image_utils = struct(
    current_target = _current_target,
    wrap_bash_build_in_common_boilerplate =
        _wrap_bash_build_in_common_boilerplate,
)
