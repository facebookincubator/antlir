# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
WATCH OUT: This has no dedicated tests.  That's mostly because of laziness,
but also because at present it's meant to only be used as part of
constructing build appliances (and similar), and the functionality that this
provides **is** tested in `test_update_shadowed`.  If this gets more complex
in logic, or more broad in usage, it's worth testing it more explicitly.

Here's why this exists:

  - In our build containers, we want to transparently provide
    repo-deterministic versions of package managers like `yum` and `dnf`.
    This is accomplished by read-only bind-mounting a wrapper on top of
    these system executables.  The wrapper configures the underlying
    programs to talk to a repo-deterministic package repo server proxy, and
    provides other measures to make them behave more safely and sanely.

  - In rare cases (e.g. FB Chef usage), the build container wants to
    upgrade the installed version of `yum` using the installed `yum`.  While
    this a Very Interesting Pattern (TM), supporting it is easier than
    changing the status quo.

  - When the package manager is shadowed, its attempt to overwrite itself
    will fail (Device or resource busy).  We mitigate this by using
    `LD_PRELOAD` to redirect its write (this uses `rename` from `glibc`).

  - This `.bzl` here ensures that the `.so` to be preloaded is built with
    a toolchain that is compatible with the package manager itself.
    Specifically, we don't want to use the outer Buck to build this.

More discussion of the problem space and other approaches can be found in
the commit message for D21390244.
"""

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def image_build_librename_shadowed(
        name,
        parent_layer,
        **kwargs):
    "`parent_layer` must have a C compiler."

    # Build as root, since this gets installed using `image.clone`, and this
    # should be installed as owned by `root`.  Future: this is a use-case
    # for `feature.install` supporting installs from image sources.
    user = "root"
    setup_layer = "SETUP__image_build_librename_shadowed__" + name
    image.layer(
        name = setup_layer,
        parent_layer = parent_layer,
        features = [
            feature.ensure_subdirs_exist("/", "build", user = user),
            feature.install(
                "//antlir/bzl/genrule/librename_shadowed:rename_shadowed.c",
                "/build/rename_shadowed.c",
            ),
        ],
        **kwargs
    )
    image.genrule_layer(
        name = name,
        # Keep this in sync with the `cc` call in `TARGETS`.
        cmd = [
            "cc",
            "-shared",
            "-o",
            "/build/librename_shadowed.so",
            "-Wall",
            "-Werror",
            "-O2",
            "-fvisibility=hidden",
            "-ldl",
            "-fPIC",
            "/build/rename_shadowed.c",
        ],
        parent_layer = ":" + setup_layer,
        rule_type = "build_librename_shadowed",
        user = user,
        antlir_rule = "user-internal",
        **kwargs
    )
