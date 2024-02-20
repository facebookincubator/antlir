# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_defs.bzl", "config", "repository_name", "target_utils")

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

def antlir_dep(target):
    """Get a normalized target referring to a dependency under the root Antlir
    directory. This helper should be used when referring to any Antlir target,
    excluding `load` statements. This should not be used when referring to
    targets defined outside of the Antlir directory, e.g. user-defined layers in
    external build files.

    For example, if you want to refer to $cell//antlir:compiler, the dependency
    should be expressed as `antlir_dep(":compiler")`. Similarly, if you want to
    refer to $cell//antlir/nspawn_in_subvol:run, the dependency should be
    expressed as `antlir_dep("nspawn_in_subvol:run")`.

    Specifically, this will add the Antlir cell name, and the "antlir" prefix
    to the package path, which will ensure the target is resolved correctly and
    will help when moving Antlir to its own cell."""

    if "//" in target or target.startswith("/"):
        fail(
            "Antlir deps should be expressed as a target relative to the " +
            "root Antlir directory, e.g. instead of `$cell//antlir/foo:bar` " +
            "the dep should be expressed as `foo:bar`.",
        )

    if target.startswith(":"):
        return "{}//antlir{}".format(config.get_antlir_cell_name(), target)
    return "{}//antlir/{}".format(config.get_antlir_cell_name(), target)
