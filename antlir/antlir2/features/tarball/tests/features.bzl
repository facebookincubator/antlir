# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")

def tarball_feature_contents(prefix):
    return [
        feature.ensure_dirs_exist(dirs = paths.join(prefix, "foo")),
        feature.install_text(
            dst = paths.join(prefix, "foo/bar"),
            mode = "a+r,u+ws",
            text = "I am bar\n",
        ),
        feature.ensure_dirs_exist(dirs = paths.join(prefix, "foo/baz")),
        feature.install_text(
            dst = paths.join(prefix, "foo/baz/qux"),
            mode = "a+r,u+w",
            text = "I am qux\n",
        ),
        feature.install_text(
            dst = paths.join(prefix, "foo/owned"),
            group = 1000,
            mode = "a+r,u+w",
            text = "I'm owned by antlir\n",
            user = 1000,
        ),
    ]
