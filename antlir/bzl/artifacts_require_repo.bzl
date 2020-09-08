# Some Buck modes build non-portable artifacts that MUST be executed out
# of the original repo.
def _built_artifacts_require_repo():
    cpp_lib = native.read_config("defaults.cxx_library", "type")
    python_pkg = native.read_config("python", "package_style")
    return (cpp_lib == "shared" or python_pkg == "inplace")

ARTIFACTS_REQUIRE_REPO = _built_artifacts_require_repo()
