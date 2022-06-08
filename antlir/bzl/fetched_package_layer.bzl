# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
We want to be able to use packages of files, fetched from an external data
store, for in-repo builds. Conceptually, this is pretty simple:
  - The repo stores an address and a cryptographic hash of the package.
  - There's a build target that fetches the package, checks the hash, and
    presents the package contents as an `image.layer`.

The above process is repo-hermetic (unless the package is unavailable), and
lets repo builds use pre-built artifacts. Such artifacts bring two benefits:
  - Speed: we can explicitly cache large, infrequently changed artifacts.
  - Controlled API churn: I may want to use a "stable" version of my
    dependency, rather than whatever I might build off trunk.

The details of "how to fetch a package" will vary depending on the package
store. This is abstracted by `_PackageFetcherInfo` below.

The glue code in this file specifies a uniform way of exposing package
stores as Buck targets. Its opinions are, roughly:

  - The data store has many versions of the package, each one immutable.
    New versions keep getting added.  However, a repo checkout may only
    access some fixed versions of a package via "tags".  Each (package, tag)
    pair results in a completely repo-deterministic `image.layer`.

  - In the repo, a (package, tag) pair is an `image.layer` target in a
    centralized repo location.

  - The layer's mount info has a correctly populated `runtime_source`, so if
    another layer mounts it at build-time, then this mount can be replicated
    at runtime.

  - One representation is provided for the in-repo package database:
    merge-conflict-free "json dir".  Aside: we used to have a "db.bzl"
    format, which was cheaper at Buck parse-time, but prone to merge
    conflicts.  We abandoned it.  In contrast, the "json dir" DB format is
    free of merge conflicts, so long as each package-tag pair is only update
    by one actor.

You can use a DB by putting something like the below in a Buck file.  Prefer
to put this in a separate directory that will NOT get built by humans
running `buck build project/...` or by CI, since fetching all packages on
every build is a potentially expensive, and definitely wasteful, operation.

    # In `pkg/pkg.bzl` --

    def _fetched_layer(name, tag = "stable"):
        return "//pkg/db:" + name + "/" + tag + "-USE-pkg.fetched_layer"
    pkg = struct(fetched_layer = _fetched_layer)

    # In `pkg/db/package_name/tag_name.json` --

    # Two MANDATORY lines of comments on how to update this file.
    # At FB, this points at the internal "update package DB" script.
    {"address": ..., "hash", ...}

    # In `pkg/db/TARGETS` --

    fetched_package_layers_from_json_dir_db(
        fetcher = {
            "fetch_package": "downloads package to $1 and writes its filename to stdout",
            "print_package_feature": "writes `tarball`/`install_files` JSON to stdout",
            "print_mount_config": "adds package address to `runtime_source`",
        },
        package_db_dir = "db/",
        layer_suffix = "-USE-pkg.fetched_layer",
        # For some advanced use-cases, you may want to also specify
        # `nondeterministic_fs_metadata_suffix`, see its inline docs.
    )

Now you can refer to a stable version of a package, represented as an
`image.layer`, via `pkg.fetched_layer("name")`.
"""

load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "export_file", "get_visibility")
load("//antlir/bzl/image/feature:new.bzl", "private_do_not_use_feature_json_genrule")
load(":constants.bzl", "REPO_CFG")
load(":image_layer.bzl", "image_layer")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")

_PackageFetcherInfo = provider(fields = [
    # This executable target downloads the package to $1 and
    # prints its filename.
    "fetch_package",
    # This executable target prints a feature JSON responsible for
    # configuring the entire layer to represent the fetched package,
    # including file data, owner, mode, etc.
    #
    # See each fetcher's in-source docblock for the details of its contract.
    "print_package_feature",
    # The analog of `fetch_package` for `nondeterministic_fs_metadata_suffix`.
    # Ought to behave the same as `fetch_package` as far as reasonably
    # possible (of course, VFS metadata like ownership cannot be faithfully
    # represented in `buck-out`).
    #
    # This fetcher should define its contract in the docblock of the
    # target's main source file.
    "fetch_with_nondeterministic_fs_metadata",
    # An executable target that defines `runtime_source` and
    # `default_mountpoint` for the `mount_config` of the package layer.
    "print_mount_config",
])

# Read the doc-block for the purpose and high-level usage.
def fetched_package_layers_from_json_dir_db(
        # Path to a database directory inside the current project (i.e.
        # relative to the parent of your TARGETS file).
        package_db_dir,
        # Dict of `_PackageFetcherInfo` kwargs
        fetcher,
        # Layer targets will have the form `<package>/<tag><suffix>`.
        # See `def _fetched_layer` in the docblock for the intended usage.
        layer_suffix,
        # If set, also create `<package>/<tag><suffix>` targets that have
        # the package contents as a regular directory output in `buck-out`.
        # Note that these packages cannot support things other than plain
        # files or directories, ownership, or most other VFS metadata
        # ("executable by user" is likely the only `stat` field you can rely
        # on).  The contract of "how the package is presented" may also
        # differ in other significant ways from fetched layers -- this is
        # entirely up to the fetcher's design, so tread carefully.
        nondeterministic_fs_metadata_suffix = None,
        visibility = None):
    fetcher = _PackageFetcherInfo(**fetcher)

    # Normalizing lets us treat `package_dir_db` as a prefix.  It also
    # avoids triggering a bug in Buck, causing it to silently abort when a
    # glob pattern starts with `./`.
    package_db_prefix = paths.normalize(package_db_dir) + "/"
    suffix = ".json"
    for p in native.glob([package_db_prefix + "*/*" + suffix]):
        if not p.startswith(package_db_prefix) or not p.endswith(suffix):
            fail("Bug: {} was not {}*/*{}".format(p, package_db_prefix, suffix))
        package, tag = p[len(package_db_prefix):-len(suffix)].split("/")
        export_file(name = p, antlir_rule = "user-internal")
        print_how_to_fetch_json = _print_how_to_fetch_json(":" + p)
        _fetched_package_layer(
            package = package,
            tag = tag,
            name_suffix = layer_suffix,
            print_how_to_fetch_json = print_how_to_fetch_json,
            fetcher = fetcher,
            visibility = visibility,
        )
        if nondeterministic_fs_metadata_suffix != None:
            _fetched_package_with_nondeterministic_fs_metadata(
                name = package + "/" + tag + nondeterministic_fs_metadata_suffix,
                package = package,
                print_how_to_fetch_json = print_how_to_fetch_json,
                fetcher = fetcher,
                visibility = visibility,
            )

# Takes one of two options:
#   - A JSONable dict describing how to fetch the package instance.
#   - A string path to a target whose output has a comment on the
#     first line, and JSON on subsequent lines.
def _print_how_to_fetch_json(how_to_fetch):
    if types.is_dict(how_to_fetch):
        return "echo " + shell.quote(structs.as_json(struct(**how_to_fetch)))
    elif types.is_string(how_to_fetch):
        return "tail -n +3 $(location {})".format(how_to_fetch)
    fail("`how_to_fetch` must be str/dict, not {}".format(how_to_fetch))

# Deliberately not usable stand-alone, use `fetched_package_layers_from_db`
# to define packages uniformly in one project.  This ensures each package is
# only fetched once.
def _fetched_package_layer(
        package,
        tag,
        name_suffix,
        print_how_to_fetch_json,
        fetcher,  # `_PackageInfoFetcher`
        visibility,
        flavor = REPO_CFG.antlir_linux_flavor):
    name = package + "/" + tag + name_suffix
    visibility = get_visibility(visibility)

    fetched_pkg_target_name = name + "-fetched-package"
    buck_genrule(
        name = fetched_pkg_target_name,
        out = "out",
        # Uncacheable for the same reasons as _fetched_package_with_nondeterministic_fs_metadata
        cacheable = False,
        bash = """
        # {deps}
        set -ue -o pipefail
        mkdir -p "$OUT"/pkg
        printf "pkg/" > "$OUT"/fetched_pkg_name.txt
        {print_how_to_fetch_json} |
            $(exe {fetch_package}) {quoted_package} "$OUT"/pkg >> "$OUT"/fetched_pkg_name.txt
        """.format(
            deps = [
                # We want to re-fetch packages if the fetching mechanics change.
                # `def fake_macro_library` has more details.
                "//antlir/bzl:fetched_package_layer",
            ],
            fetch_package = fetcher.fetch_package,
            print_how_to_fetch_json = print_how_to_fetch_json,
            quoted_package = shell.quote(package),
        ),
        antlir_rule = "user-internal",
        labels = ["uses_fbpkg"],
    )

    package_feature = name + "-fetched-package-feature"
    private_do_not_use_feature_json_genrule(
        name = package_feature,
        deps = [
            # We want to re-fetch packages if the fetching mechanics change.
            # `def fake_macro_library` has more details.
            "//antlir/bzl:fetched_package_layer",
            ":" + fetched_pkg_target_name,
        ],
        output_feature_cmd = """
        {print_how_to_fetch_json} |
            $(exe {print_package_feature}) \
                {quoted_package} {quoted_target} \
                $(location {fetched_pkg_target}) \
                {fetched_pkg_target} > "$OUT"
        """.format(
            print_package_feature = fetcher.print_package_feature,
            quoted_package = shell.quote(package),
            quoted_target = shell.quote(normalize_target(":" + name)),
            print_how_to_fetch_json = print_how_to_fetch_json,
            fetched_pkg_target = shell.quote(normalize_target(":" + fetched_pkg_target_name)),
        ),
        visibility = visibility,
    )

    mount_config = name + "-fetched-package-mount-config"
    buck_genrule(
        name = mount_config,
        bash = '''
        {print_how_to_fetch_json} |
            $(exe {print_mount_config}) {quoted_package} > "$OUT"
        '''.format(
            print_mount_config = fetcher.print_mount_config,
            quoted_package = shell.quote(package),
            print_how_to_fetch_json = print_how_to_fetch_json,
        ),
        antlir_rule = "user-internal",
    )

    image_layer(
        name = name,
        flavor = flavor,
        features = [":" + package_feature],
        mount_config = ":" + mount_config,
        visibility = visibility,
        # Useful for queries on leaf image layers to determine the packages
        # being fetched throughout the image layer stack
        labels = [
            "antlir_fetched_package__name={}".format(package),
            "antlir_fetched_package__tag={}".format(tag),
        ],
    )

# Deliberately not usable stand-alone, use `fetched_package_layers_from_db`
# to define packages uniformly in one project.  This ensures each package is
# only fetched once.
def _fetched_package_with_nondeterministic_fs_metadata(
        name,
        package,
        print_how_to_fetch_json,
        fetcher,  # `_PackageInfoFetcher`
        visibility):
    buck_genrule(
        name = name,
        # (i) Fbpkg is essentially a cache, it's reasonably fast. No need
        #     to burn RAM cache on this.
        # (ii) Fbpkgs are often huge, and would cache poorly in the Buck
        #      caches -- historically they've used a chunked design, which
        #      had a hard time reconstituting artifacts larger than a couple
        #      hundred megs.
        cacheable = False,
        bash = '''
        mkdir "$OUT" && {print_how_to_fetch_json} |
            $(exe {fetch_with_nondeterministic_fs_metadata}) \
                {quoted_package} "$OUT"
        '''.format(
            fetch_with_nondeterministic_fs_metadata = fetcher.fetch_with_nondeterministic_fs_metadata,
            quoted_package = shell.quote(package),
            print_how_to_fetch_json = print_how_to_fetch_json,
        ),
        type = "fetched_package_with_nondeterministic_fs_metadata",
        visibility = get_visibility(visibility),
        antlir_rule = "user-internal",
        labels = ["uses_fbpkg"],
    )
