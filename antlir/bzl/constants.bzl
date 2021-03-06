# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This combines configurable build-time constants (documented on REPO_CFG
# below), and non-configurable constants that are currently not namespaced.
#
# Note that there's no deep reason for this struct / non-struct split, so we
# could easily move everything into the struct.
#
load("//antlir/bzl:oss_shim.bzl", "do_not_use_repo_cfg")
load("//antlir/bzl:shape.bzl", "shape")

DO_NOT_USE_BUILD_APPLIANCE = "__DO_NOT_USE_BUILD_APPLIANCE__"
VERSION_SET_ALLOW_ALL_VERSIONS = "__VERSION_SET_ALLOW_ALL_VERSIONS__"

# This needs to be kept in sync with
# `antlir.nspawn_in_subvol.args._QUERY_TARGETS_AND_OUTPUTS_SEP`
QUERY_TARGETS_AND_OUTPUTS_SEP = "|"

# Use `_get_optional_str_cfg` or `_get_str_list_cfg` instead.
def _do_not_use_directly_get_cfg(name, default = None):
    # Allow `buck -c` overrides from the command-line
    val = native.read_config("antlir", name)
    if val != None:
        return val

    val = do_not_use_repo_cfg.get(name)
    if val != None:
        return val

    return default

# We don't have "globally required" configs because code that requires a
# config will generally loudly fail on a config value that is None.
def _get_optional_str_cfg(name, default = None):
    return _do_not_use_directly_get_cfg(name, default = default)

# Defaults to the empty list if the config is not set.
#
# We use space to separate plurals because spaces are not allowed in target
# paths, and also because that's what `.buckconfig` is supposed to support
# for list configs (but does not, due to bugs).
def _get_str_list_cfg(name, separator = " ", default = None):
    s = _do_not_use_directly_get_cfg(name)
    return s.split(separator) if s else (default or [])

# Defaults to the empty list if the config is not set
def _get_version_set_to_path():
    lst = _get_str_list_cfg("version_set_to_path")
    vs_to_path = dict(zip(lst[::2], lst[1::2]))

    if 2 * len(vs_to_path) != len(lst):
        fail("antlir.version_set_to_path is a space-separated dict: k1 v1 k2 v2")

    # A layer can turn off version locking
    # via `version_set = VERSION_SET_ALLOW_ALL_VERSIONS`.
    vs_to_path[VERSION_SET_ALLOW_ALL_VERSIONS] = "TROLLING TROLLING TROLLING"
    return vs_to_path

# Defaults to the empty list if the config is not set
def _get_artifact_key_to_path():
    lst = _get_str_list_cfg("artifact_key_to_path")
    key_to_path = dict(zip(lst[::2], lst[1::2]))

    if 2 * len(key_to_path) != len(lst):
        fail("antlir.artifact_key_to_path is a space-separated dict: k1 v1 k2 v2")

    return key_to_path

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
    build_appliance_default = str,
    host_mounts_allowed_in_targets = shape.field(shape.list(str), optional = True),
    host_mounts_for_repo_artifacts = shape.field(shape.list(str), optional = True),
    rpm_installer_default = str,
    rpm_installers_supported = shape.list(str),
    version_set_to_path = shape.dict(str, str),
    version_set_default = str,
)

REPO_CFG = shape.new(
    repo_config_t,
    # This one is not using the access methods to provide the precedence order
    # because the way this is determined is *always* based on the build mode
    # provided, ie `@mode/opt` vs `@mode/dev`.  And the build mode provided
    # determines the value of the `.buckconfig` properties used. There is no
    # way to override this value except to use a different build mode.
    artifacts_require_repo = (
        (native.read_config("defaults.cxx_library", "type") == "shared") or
        (native.read_config("python", "package_style") == "inplace")
    ),

    # This is a dictionary that allow for looking up configurable artifact
    # targets by a key.
    artifact = _get_artifact_key_to_path(),

    # The target path of the build appliance used for `image.layer`s that do
    # not specify one via `build_opts`.
    build_appliance_default = _get_optional_str_cfg("build_appliance_default"),

    # At FB, the Antlir team tightly controls the usage of host mounts,
    # since they are a huge footgun, and are a terrible idea for almost
    # every application.  To create an easy-to-review code bottleneck, any
    # feature target using a host-mount must be listed in this config.
    host_mounts_allowed_in_targets = _get_str_list_cfg("host_mounts_allowed_in_targets"),
    # Enumerates host mounts required to execute FB binaries in @mode/dev.
    #
    # This is turned into json and loaded by the python side of the
    # `nspawn_in_subvol` sub system.  In the future this would be
    # implemented via a `Shape` so that the typing can be maintained across
    # bzl/python.
    host_mounts_for_repo_artifacts = _get_str_list_cfg(
        "host_mounts_for_repo_artifacts",
    ),

    # Whether RPMs are installed with `yum` or `dnf` by default.  When using
    # a non-default build appliance, you will usually also want to override
    # this via your layer's `build_opts`.
    rpm_installer_default = _get_optional_str_cfg("rpm_installer_default"),
    # Which RPM installers are supported. Internally antlir supports both dnf
    # and yum, but the OSS appliance(s) are more modern and only support dnf.
    rpm_installers_supported = _get_str_list_cfg("rpm_installers_supported", default = ["dnf", "yum"]),

    # TODO(mpatlasov,lesha): add docs.  This feature is in development, and
    # should not be used yet.
    #
    # KEEP THIS DICTIONARY SMALL.
    #
    # For each `image.feature`, we have to emit as many targets as there are
    # elements in this list, because we do not know the version set that the
    # including `image.layer` will use.  This would be fixable if Buck
    # supported providers like Bazel does.
    version_set_to_path = _get_version_set_to_path(),
    version_set_default = _get_optional_str_cfg(
        "version_set_default",
        default = VERSION_SET_ALLOW_ALL_VERSIONS,
    ),
)
