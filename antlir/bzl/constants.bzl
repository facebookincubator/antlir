# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "config", "do_not_use_repo_cfg")
load(":constants.shape.bzl", "flavor_config_t", "nevra_t", "repo_config_t")
load(":target_helpers.bzl", "normalize_target")

CONFIG_KEY = "antlir"

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
def _get_artifact_key_to_path():
    lst = _get_str_list_cfg("artifact_key_to_path")
    key_to_path = dict(zip(lst[::2], lst[1::2]))

    if 2 * len(key_to_path) != len(lst):
        fail("antlir.artifact_key_to_path is a space-separated dict: k1 v1 k2 v2")

    return key_to_path

def new_nevra(**kwargs):
    return nevra_t(**kwargs)

def new_flavor_config(
        name,
        build_appliance,
        **kwargs):
    """
    Arguments

    - `name`: The name of the flavor
    - `build_appliance`: Path to a layer target of a build appliance,
    containing an installed `rpm_repo_snapshot()`, plus an OS image
    with other image build tools like `btrfs`, `dnf`, `yum`, `tar`, `ln`, ...
    """
    if build_appliance == None:
        fail(
            "Must be a target path, or a value from `constants.bzl`",
            "build_appliance",
        )

    if build_appliance:
        build_appliance = normalize_target(build_appliance)

    return flavor_config_t(
        name = name,
        build_appliance = build_appliance,
        **kwargs
    )

def _get_flavor_to_config():
    flavor_to_config = {}
    for flavor, orig_flavor_config in do_not_use_repo_cfg.get("flavor_to_config", {}).items():
        flavor_config = {"name": flavor}
        flavor_config.update(orig_flavor_config)  # we'll mutate a copy
        flavor_to_config[flavor] = new_flavor_config(**flavor_config)

    return flavor_to_config

def use_rc_target(*, target, exact_match = False):
    target = normalize_target(target)
    if not exact_match and REPO_CFG.rc_targets == ["all"]:
        return True

    return target in REPO_CFG.rc_targets

REPO_CFG = repo_config_t(
    artifacts_require_repo = (
        (native.read_config("defaults.cxx_library", "type") == "shared") or
        (native.read_config("python", "package_style") == "inplace")
    ) and native.read_config("antlir", "require_repo", "true") == "true",

    # This is a dictionary that allow for looking up configurable artifact
    # targets by a key.
    artifact = _get_artifact_key_to_path(),

    # Enumerates host mounts required to execute FB binaries in @mode/dev.
    #
    # This is turned into json and loaded by the python side of the
    # `nspawn_in_subvol` sub system.  In the future this would be
    # implemented via a `Shape` so that the typing can be maintained across
    # bzl/python.
    host_mounts_for_repo_artifacts = _get_str_list_cfg(
        "host_mounts_for_repo_artifacts",
    ),
    flavor_to_config = _get_flavor_to_config(),
    # KEEP THIS DICTIONARY SMALL.
    #
    # For each `feature`, we have to emit as many targets as there are
    # elements in this list, because we do not know the version set that the
    # including `image.layer` will use.  This would be fixable if Buck
    # supported providers like Bazel does.
    antlir_linux_flavor = _get_str_cfg("antlir_linux_flavor", allow_none = True),
    antlir_cell_name = config.get_antlir_cell_name(),
    rc_targets = [
        (t if t == "all" else normalize_target(t))
        for t in _get_str_list_cfg("rc_targets", separator = ",")
    ],
    flavor_alias = _get_str_cfg("flavor-alias", allow_none = True),
)
