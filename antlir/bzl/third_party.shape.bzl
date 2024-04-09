# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

script_t = record(
    prepare = field(str | None, default = None),
    build = str,
    install = str,
    patches = field(list[str] | None, default = None),
)

dep_t = record(
    name = str,
    source = str,
    paths = list[str],
)
