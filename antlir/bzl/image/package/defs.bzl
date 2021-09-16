# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the package.* macros."

load(":new.bzl", "package_new")

package = struct(
    new = package_new,
)
