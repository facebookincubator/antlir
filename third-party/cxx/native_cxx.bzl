load("@bazel_skylib//lib:paths.bzl", "paths")

# we disable implicit native rules by default for antlir things, but for
# third-party cxx rules it is easier to just use the buck native ruleset
cxx_library = native.cxx_library
prebuilt_cxx_library = native.prebuilt_cxx_library

def subdir_glob(glob_specs):
    # Simple re-implementation of the Buck Python DSL subdir_glob
    # (https://buck.build/function/subdir_glob.html) which does not exist in
    # the Starlark DSL, but is essential for third party cxx deps. This is a
    # fairly naive implementation (for example, it does not ensure that there
    # are no conflicting files), but is good enough for the existing use case
    # of gtest (and likely only it will only ever be used for that)
    srcs = {}
    for subdir, pattern in glob_specs:
        files = native.glob([paths.join(subdir, pattern)])
        for f in files:
            srcs[paths.relativize(f, subdir)] = f

    return srcs
