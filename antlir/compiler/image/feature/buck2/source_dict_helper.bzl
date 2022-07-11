# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load("//antlir/bzl:wrap_runtime_deps.bzl", "maybe_wrap_executable_target")

def mark_path(target, is_layer = False):
    return {("__BUCK_LAYER_TARGET" if is_layer else "__BUCK_TARGET"): normalize_target(target)}

def unwrap_path(path_dict, is_layer = False):
    return path_dict["__BUCK_LAYER_TARGET" if is_layer else "__BUCK_TARGET"]

def _mark_path_and_get_target(source_dict, key, is_layer = False):
    if type(source_dict[key]) == dict:
        fail("Path already marked for `{key}` in `{source_dict}`".format(
            key = key,
            source_dict = source_dict,
        ))
    source_dict[key] = mark_path(source_dict[key], is_layer)
    normalized_target = unwrap_path(source_dict[key], is_layer)
    return normalized_target

def normalize_target_and_mark_path_in_source_dict(source_dict, **kwargs):
    """
    Adds tag to target at `source_dict[{source,layer,generator}}]` and
    normalizes target so target can be converted to path in
    items_for_features.py.
    """
    if not (source_dict.get("source") or
            source_dict.get("generator") or
            source_dict.get("layer")):
        fail("One of source, generator, layer must contain a target")

    if source_dict.get("source"):
        normalized_target = _mark_path_and_get_target(source_dict, "source")

        if kwargs.get("is_buck_runnable"):
            was_wrapped, source_dict["source"] = maybe_wrap_executable_target(
                target = unwrap_path(source_dict["source"]),
                wrap_suffix = "install_buck_runnable_wrap_source",
                visibility = None,
                # NB: Buck makes it hard to execute something out of an
                # output that is a directory, but it is possible so long as
                # the rule outputting the directory is marked executable
                # (see e.g. `print-ok-too` in `feature_install_files`).
                path_in_output = source_dict.get("path", None),
                runs_in_build_steps_causes_slow_rebuilds =
                    kwargs.get("runs_in_build_steps_causes_slow_rebuilds"),
            )
            if was_wrapped:
                # The wrapper above has resolved `source_dict["path"]`, so the
                # compiler does not have to.
                source_dict["path"] = None
            normalized_target = _mark_path_and_get_target(source_dict, "source")

    elif source_dict.get("generator"):
        _was_wrapped, source_dict["generator"] = maybe_wrap_executable_target(
            target = source_dict["generator"],
            wrap_suffix = "image_source_wrap_generator",
            visibility = [],  # Not visible outside of project
            # Generators run at build-time, that's the whole point.
            runs_in_build_steps_causes_slow_rebuilds = True,
        )
        normalized_target = _mark_path_and_get_target(source_dict, "generator")

    else:
        normalized_target = _mark_path_and_get_target(
            source_dict,
            "layer",
            is_layer = True,
        )

    return source_dict, normalized_target

def is_build_appliance(target):
    return target in {
        config.build_appliance: 1
        for _, config in REPO_CFG.flavor_to_config.items()
    }
