load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def generators(name, rel_star_paths):
    feature.new(
        name = name,
        features = [
            feature.ensure_subdirs_exist("/usr/lib/metalos", "generators"),
        ] + [
            feature.install(
                src,
                "/usr/lib/metalos/generators/{}".format(paths.basename(src)),
            )
            for src in rel_star_paths
        ],
        visibility = ["//metalos/..."],
    )
