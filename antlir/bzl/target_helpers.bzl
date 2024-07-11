# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_defs.bzl", "repository_name", "target_utils")

def normalize_target(target):
    if target.startswith("//"):
        return repository_name()[1:] + target

    # Don't normalize if already normalized. This avoids the Buck error:
    #   Error in package_name: Top-level invocations of package_name are not
    #   allowed in .bzl files.  Wrap it in a macro and call it from a BUCK file.
    if "//" in target:
        return target

    parsed = target_utils.parse_target(
        target,
        # The repository name always starts with "@", which we do not want here.
        # default_repo will be empty for the main repository, which matches the
        # results from $(query_targets ...).
        # @lint-ignore BUCKLINT
        default_repo = repository_name()[1:],
        # @lint-ignore BUCKLINT
        default_base_path = native.package_name(),
    )
    return target_utils.to_label(
        repo = parsed.repo,
        path = parsed.base_path,
        name = parsed.name,
    )
