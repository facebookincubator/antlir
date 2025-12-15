# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("@fbsource//tools/build_defs:feature_rollout_utils.bzl", "rollout")
load("@prelude//linking:shared_libraries.bzl", "traverse_shared_library_info")
load("@prelude//python:python.bzl", "PythonLibraryInfo")
load("//antlir/bzl:oss_shim.bzl", "rollout", read_bool = "ret_false") # @oss-enable

PYTHON_OUTPLACE_PAR_ROLLOUT = rollout.create_feature(
    {
        # "example_opt_in": True,
        "antlir/antlir2/features/install/tests": True,
        "antlir/antlir2/features/install/tests/fb": False,
        # @oss-disable[end= ]: "fblite/devx/fixmydevenv": True,
        # @oss-disable[end= ]: "python/pylot": True,
        # @oss-disable[end= ]: "registry/builder/rpm/bzl/mac_sign/tests": True,
    },
)

def is_python_target(target) -> bool:
    return "library-info" in target[DefaultInfo].sub_targets and PythonLibraryInfo in target.sub_target("library-info")

def is_python_xar_target(target) -> bool:
    """
    Returns whether the given target is a python xar archive.
    """
    info = _extract_python_library_info(target)
    if info:
        return info.par_style == "xar"
    return False

def _extract_python_library_info(target) -> PythonLibraryInfo | None:
    """
    Extracts the PythonLibraryInfo from a target.
    """
    library_info = target[DefaultInfo].sub_targets.get("library-info")
    if library_info:
        return library_info.get(PythonLibraryInfo)
    return None

def _extract_native_python_executable(target) -> DefaultInfo | None:
    """
    Extracts the par executable from a target. For native python this is the actual
    native executable that doubles as the interpreter.
    """
    native_executable = target[DefaultInfo].sub_targets.get("native-executable")
    if native_executable:
        return native_executable[DefaultInfo]
    return None

def _extract_omnibus(target) -> DefaultInfo | None:
    """
    Extracts libomnibus output (if present) from a target. Only relevant if the python
    binary was linked with omnibus linking.
    """
    omnibus = target[DefaultInfo].sub_targets.get("omnibus")
    if omnibus:
        return omnibus[DefaultInfo]
    return None

def extract_par_elfs(target) -> list[Artifact]:
    """
    Extracts the following from the given target (assumed to be a python binary):

      1. The artifact representing the actual executable. For native python, this
         is the interpreter.
      2. The shared libraries provided by the distro platform. First-party shlibs
         are ignored.

    These are then passed on to the automated find-requires to make sure rpms
    end up in the container to satisfy shlib deps.
    """
    elfs = []
    python_info = _extract_python_library_info(target)
    if python_info:
        elfs.extend([
            shlib.lib.output
            for shlib in (
                traverse_shared_library_info(python_info.shared_libraries, transformation_provider = None) +
                traverse_shared_library_info(python_info.extension_shared_libraries, transformation_provider = None)
            )
            if isinstance(shlib.lib.output, Artifact)
        ])

    for info in (_extract_native_python_executable(target), _extract_omnibus(target)):
        if info and info.default_outputs:
            elfs.append(info.default_outputs[0])

    return elfs
