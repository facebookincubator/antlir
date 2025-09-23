# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
load("@prelude//linking:shared_libraries.bzl", "traverse_shared_library_info")
load("@prelude//python:python.bzl", "PythonLibraryInfo")
load("//antlir/bzl:oss_shim.bzl", "rollout", read_bool = "ret_false") # @oss-enable

PYTHON_OUTPLACE_PAR_ROLLOUT = rollout.create_feature(
    {
        # "example_opt_in": True,
        "antlir/antlir2/features/install/tests": True,
        "antlir/antlir2/features/install/tests/fb": False,
        # @oss-disable
        # @oss-disable
        # @oss-disable
    },
)

def is_python_target(target) -> bool:
    return "library-info" in target[DefaultInfo].sub_targets and PythonLibraryInfo in target.sub_target("library-info")

def _extract_python_library_info(target) -> PythonLibraryInfo | None:
    """
    Extracts the PythonLibraryInfo from a target.
    """
    library_info = target[DefaultInfo].sub_targets.get("library-info")
    if library_info:
        return library_info.get(PythonLibraryInfo)
    return None

def _extract_par_executable_info(target) -> DefaultInfo | None:
    """
    Extracts the par executable from a target. For native python this is the actual
    native executable that doubles as the interpreter.
    """
    return target[DefaultInfo].sub_targets.get("native-executable", {}).get(DefaultInfo)

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
                traverse_shared_library_info(python_info.shared_libraries) +
                traverse_shared_library_info(python_info.extension_shared_libraries)
            )
            if isinstance(shlib.lib.output, Artifact)
        ])

    par_executable_info = _extract_par_executable_info(target)
    if par_executable_info:
        exe = par_executable_info.default_outputs
        if exe:
            elfs.append(exe[0])

    return elfs
