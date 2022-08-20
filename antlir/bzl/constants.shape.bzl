# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

bzl_const_t = shape.shape(
    layer_feature_suffix = str,
    PRIVATE_feature_suffix = str,
    version_set_allow_all_versions = str,
    hostname_for_compiler_in_ba = str,
)

nevra_t = shape.shape(
    name = shape.field(str),
    # TODO: Codemod all callsites and update this to be `int`.
    epoch = shape.field(str),
    version = shape.field(str),
    release = shape.field(str),
    arch = shape.field(str),
)

# These are configuration keys that can be grouped under a specific common
# name called flavor.  This way, during run-time, we can choose default
# values for set of configuration keys based on selected flavor name.
flavor_config_t = shape.shape(
    name = shape.field(str),
    # FIXME: Ideally, remove `optional = True`.  This field is not optional,
    # per `new_flavor_config` below, but expressing that requires changing
    # the wire format for `DO_NOT_USE_BUILD_APPLIANCE` to be a string
    # instead of `None` -- see `new_flavor_config`. This needs a Python fix.
    build_appliance = shape.field(str, optional = True),
    rpm_installer = shape.field(str, optional = True),
    rpm_repo_snapshot = shape.field(str, optional = True),
    apt_repo_snapshot = shape.field(shape.list(str), optional = True),
    version_set_path = shape.field(str, optional = True),
    rpm_version_set_overrides = shape.field(shape.list(nevra_t), optional = True),
    unsafe_bypass_flavor_check = shape.field(bool, optional = True),
)

#
# These are repo-specific configuration keys, which can be overridden via
# the Buck CLI for debugging / development purposes.
#
# We do not want to simply use `.buckconfig` for these, because in FBCode,
# the CI cost to updating `.buckconfig` is quite high (every project
# potentially needs to be tested and rebuilt).
#
# Instead, we keep the per-repo configuration in `oss_shim_impl.bzl`, and
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
#   - `do_not_use_repo_cfg` loaded via `oss_shim.bzl`
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
    artifact = shape.dict(str, str),
    host_mounts_allowed_in_targets = shape.list(shape.path),
    host_mounts_for_repo_artifacts = shape.list(shape.path),
    # This holds the default flavors that a feature should cover.
    # Compared to `flavor_to_config`, it does not contain the
    # `antlir_test` flavor, which shouldn't be always defined.
    flavor_available = shape.list(str),
    stable_flavors = shape.list(str),
    flavor_default = str,
    flavor_to_config = shape.dict(str, flavor_config_t),
    ba_to_flavor = shape.dict(str, str),
    antlir_linux_flavor = str,
    antlir_cell_name = str,
    rc_targets = shape.list(str),
)
