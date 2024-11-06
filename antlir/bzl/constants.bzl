# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "do_not_use_repo_cfg")
load(":constants.shape.bzl", "repo_config_t")
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

# Defaults to the empty list if the config is not set.
#
# We use space to separate plurals because spaces are not allowed in target
# paths, and also because that's what `.buckconfig` is supposed to support
# for list configs (but does not, due to bugs).
def _get_str_list_cfg(name, separator = " ", default = None):
    s = _do_not_use_directly_get_cfg(name)
    return s.split(separator) if s else (default or [])

def use_rc_target(*, target, exact_match = False):
    target = normalize_target(target)
    if not exact_match and REPO_CFG.rc_targets == ["all"]:
        return True

    return target in REPO_CFG.rc_targets

REPO_CFG = repo_config_t(
    # Enumerates host mounts required to execute FB binaries in @mode/dev.
    host_mounts_for_repo_artifacts = _get_str_list_cfg(
        "host_mounts_for_repo_artifacts",
    ),
    rc_targets = [
        (t if t == "all" else normalize_target(t))
        for t in _get_str_list_cfg("rc_targets", separator = ",")
    ],
)
