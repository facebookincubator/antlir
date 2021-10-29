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
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load(":snapshot_install_dir.bzl", "RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR", "snapshot_install_dir")
load(":target_helpers.bzl", "normalize_target")

DO_NOT_USE_BUILD_APPLIANCE = "__DO_NOT_USE_BUILD_APPLIANCE__"
CONFIG_KEY = "antlir"

BZL_CONST = shape.new(
    shape.shape(
        layer_feature_suffix = str,
        PRIVATE_feature_suffix = str,
        version_set_allow_all_versions = str,
    ),
    layer_feature_suffix = "__layer-feature",
    # Do NOT use this outside of Antlir internals.  See "Why are `feature`s
    # forbidden as dependencies?" in `bzl/image/feature/new.bzl` for a
    # detailed explanation.
    PRIVATE_feature_suffix = "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN",
    version_set_allow_all_versions = "__VERSION_SET_ALLOW_ALL_VERSIONS__",
)

def version_set_override_name(current_target):
    return "vset-override-" + sha256_b64(current_target)

# Use `_get_str_cfg` or `_get_str_list_cfg` instead.
def _do_not_use_directly_get_cfg(name, default = None):
    # Allow `buck -c` overrides from the command-line
    val = native.read_config(CONFIG_KEY, name)
    if val != None:
        return val

    val = do_not_use_repo_cfg.get(name)
    if val != None:
        return val

    return default

# We don't have "globally required" configs because code that requires a
# config will generally loudly fail on a config value that is None.
def _get_str_cfg(name, default = None, allow_none = False):
    ret = _do_not_use_directly_get_cfg(name, default = default)
    if not allow_none and ret == None:
        fail("Repo config must set key {}".format(name))
    return ret

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
    # via `version_set = BZL_CONST.version_set_allow_all_versions`.
    vs_to_path[BZL_CONST.version_set_allow_all_versions] = "TROLLING TROLLING TROLLING"
    return vs_to_path

# Defaults to the empty list if the config is not set
def _get_artifact_key_to_path():
    lst = _get_str_list_cfg("artifact_key_to_path")
    key_to_path = dict(zip(lst[::2], lst[1::2]))

    if 2 * len(key_to_path) != len(lst):
        fail("antlir.artifact_key_to_path is a space-separated dict: k1 v1 k2 v2")

    return key_to_path

_nevra_t = shape.shape(
    name = shape.field(str),
    # TODO: Codemod all callsites and update this to be `int`.
    epoch = shape.field(str),
    version = shape.field(str),
    release = shape.field(str),
    arch = shape.field(str),
)

def new_nevra(**kwargs):
    return shape.new(_nevra_t, **kwargs)

# These are configuration keys that can be grouped under a specific common
# name called flavor.  This way, during run-time, we can choose default
# values for set of configuration keys based on selected flavor name.
_flavor_config_t = shape.shape(
    name = shape.field(str),
    # FIXME: Ideally, remove `optional = True`.  This field is not optional,
    # per `new_flavor_config` below, but expressing that requires changing
    # the wire format for `DO_NOT_USE_BUILD_APPLIANCE` to be a string
    # instead of `None` -- see `new_flavor_config`. This needs a Python fix.
    build_appliance = shape.field(str, optional = True),
    rpm_installer = shape.field(str, optional = True),
    rpm_repo_snapshot = shape.field(str, optional = True),
    version_set_path = shape.field(str, optional = True),
    rpm_version_set_overrides = shape.list(_nevra_t, optional = True),
    unsafe_bypass_flavor_check = shape.field(bool, optional = True),
)

# This keeps the type private, so one cannot instantiate unvalidated flavors.
def flavor_config_t_shape_loader():
    shape.loader(
        name = "flavor_config_t",
        shape = _flavor_config_t,
        classname = "flavor_config_t",
        visibility = ["//antlir/...", "//tupperware/cm/antlir/..."],
    )

def new_flavor_config(
        name,
        build_appliance,
        rpm_installer,
        rpm_repo_snapshot = None,
        rpm_version_set_overrides = None,
        version_set_path = BZL_CONST.version_set_allow_all_versions,
        unsafe_bypass_flavor_check = False):
    """
    Arguments

    - `name`: The name of the flavor
    - `build_appliance`: Path to a layer target of a build appliance,
    containing an installed `rpm_repo_snapshot()`, plus an OS image
    with other image build tools like `btrfs`, `dnf`, `yum`, `tar`, `ln`, ...
    - `rpm_installer`: The build appliance currently does not set
    a default package manager -- in non-default settings, this
    has to be chosen per image, since a BA can support multiple
    package managers.  In the future, if specifying a non-default
    installer per image proves onerous when using non-default BAs, we
    could support a `default` symlink under `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
    - `rpm_repo_snapshot`: List of target or `/__antlir__` paths,
    see `snapshot_install_dir` doc. `None` uses the default determined
    by looking up `rpm_installer` in `RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.
    - `rpm_version_set_overrides`: List of `nevra` objects
    (see antlir/bzl/constants.bzl for definition). If rpm with given name to
    be installed, the `nevra` defines its version.
    - `unsafe_bypass_flavor_check`: Do NOT use.
    """
    if build_appliance == None:
        fail(
            "Must be a target path, or a value from `constants.bzl`",
            "build_appliance",
        )

    if rpm_installer != "yum" and rpm_installer != "dnf":
        fail("Unsupported rpm_installer supplied in build_opts")

    # When building the BA itself, we need this constant to avoid a circular
    # dependency.
    #
    # This feature is exposed a non-`None` magic constant so that callers
    # cannot get confused whether `None` refers to "no BA" or "default BA".
    if build_appliance == DO_NOT_USE_BUILD_APPLIANCE:
        build_appliance = None

    if build_appliance:
        build_appliance = normalize_target(build_appliance)

    return shape.new(
        _flavor_config_t,
        name = name,
        build_appliance = build_appliance,
        rpm_installer = rpm_installer,
        rpm_repo_snapshot = (
            snapshot_install_dir(rpm_repo_snapshot) if rpm_repo_snapshot else "{}/{}".format(
                RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR,
                rpm_installer,
            )
        ),
        rpm_version_set_overrides = rpm_version_set_overrides,
        version_set_path = version_set_path,
        unsafe_bypass_flavor_check = unsafe_bypass_flavor_check,
    )

def _get_flavor_to_config():
    flavor_to_config = {}
    for flavor, orig_flavor_config in do_not_use_repo_cfg.get("flavor_to_config", {}).items():
        flavor_config = {"name": flavor}
        flavor_config.update(orig_flavor_config)  # we'll mutate a copy

        # Apply `buck -c` overrides.
        #
        # Buck has a notion of flavors that is separate from Antlir's but
        # similar in spirit.  It uses # as the delimiter for per-flavor
        # config options, so we follow that pattern.
        config_key = CONFIG_KEY + "#" + flavor
        for key, v in flavor_config.items():
            val = native.read_config(config_key, key, None)
            if val != None:
                flavor_config[key] = val

        flavor_to_config[flavor] = new_flavor_config(**flavor_config)

    return flavor_to_config

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
    host_mounts_allowed_in_targets = shape.list(shape.path()),
    host_mounts_for_repo_artifacts = shape.list(shape.path()),
    # This holds the default flavors that a feature should cover.
    # Compared to `flavor_to_config`, it does not contain the
    # `antlir_test` flavor, which shouldn't be always defined.
    flavor_available = shape.list(str),
    flavor_default = str,
    flavor_to_config = shape.dict(str, _flavor_config_t),
    antlir_linux_flavor = str,
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
    ) and native.read_config("antlir", "require_repo", "true") == "true",

    # This is a dictionary that allow for looking up configurable artifact
    # targets by a key.
    artifact = _get_artifact_key_to_path(),

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
    flavor_available = _get_str_list_cfg("flavor_available"),
    flavor_default = _get_str_cfg("flavor_default"),
    flavor_to_config = _get_flavor_to_config(),
    # KEEP THIS DICTIONARY SMALL.
    #
    # For each `feature`, we have to emit as many targets as there are
    # elements in this list, because we do not know the version set that the
    # including `image.layer` will use.  This would be fixable if Buck
    # supported providers like Bazel does.
    antlir_linux_flavor = _get_str_cfg("antlir_linux_flavor", allow_none = True),
)
