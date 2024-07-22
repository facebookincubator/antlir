load("@prelude//rust:cargo_buildscript.bzl", _prelude_buildscript_run = "buildscript_run")
load("//antlir/bzl:build_defs.bzl", "cpp_library")

def third_party_rust_cxx_library(name, **kwargs):
    cpp_library(name = name, **kwargs)

def buildscript_run(
        name,
        buildscript_rule,
        package_name,
        version,
        **kwargs):
    filegroup_name = "{}-{}.crate".format(package_name, version)
    if not rule_exists(filegroup_name):
        # Since antlir does not vendor Rust sources, if this target doesn't
        # exist then it must be a git_fetch crate.

        # We could patch reindeer to generate this correctly since it seems to
        # know where crate roots actually exist under the repo, but I'm just
        # going to assume that it's directly in the root of the repo.
        # @lint-ignore BUCKLINT
        native.genrule(
            name = filegroup_name,
            out = ".",
            bash = """
                cp --reflink=auto -R $(location {}[sources])/*/{}/* $OUT/
            """.format(buildscript_rule, package_name),
        )

    _prelude_buildscript_run(
        name = name,
        buildscript_rule = buildscript_rule,
        package_name = package_name,
        version = version,
        **kwargs
    )
