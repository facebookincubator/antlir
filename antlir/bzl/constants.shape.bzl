# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

nevra_t = shape.shape(
    name = shape.field(str),
    # TODO: Codemod all callsites and update this to be `int`.
    epoch = shape.field(str),
    version = shape.field(str),
    release = shape.field(str),
    arch = shape.field(str),
)

#
# These are repo-specific configuration keys, which can be overridden via
# the Buck CLI for debugging / development purposes.
#
# We do not want to simply use `.buckconfig` for these, because in FBCode,
# the CI cost to updating `.buckconfig` is quite high (every project
# potentially needs to be tested and rebuilt).
#
# Instead, we keep the per-repo configuration in `build_defs_impl.bzl`, and
# the global defaults here, in `constants.bzl`.
#
# Our underlying configs use the simple type signature of `Mapping[str,
# str]` because we want to support overrides via `buck -c`.  So, some very
# simple parsing of structured configuration keys happens in this file.
#
# Configuration sources have the following precedence order:
#   - `buck -c antlir.CONFIG_NAME='foo bar'` -- note that our lists are
#     generally space-separated, so you'll want to bash quote those.
#   - `.buckconfig` -- DO NOT PUT OUR CONFIGS THERE!
#   - `do_not_use_repo_cfg` loaded via `build_defs.bzl`
#   - the defaults below -- these have to be reasonable since this is what a
#     clean open-source install will use
#
# A note on naming: please put the "topic" of the constant before the
# details, so that buildifier-required lexicographic ordering of dictionary
# keys results in related keys being grouped together.
#
#
# DANGER! ACHTUNG! PELIGRO! PERICRLRM!
# Modifications to this shape's attributes or the values in the instance
# of it below (`REPO_CFG`) could (and likely will) cause excessive
# rebuilding and incur significant build cost. These attributes and values
# are effectively global and should be treated with extreme caution.
# Don't be careless.
repo_config_t = shape.shape(
    artifacts_require_repo = bool,
    host_mounts_for_repo_artifacts = shape.list(shape.path),
    rc_targets = shape.list(str),
    flavor_alias = shape.field(str, optional = True),
)
