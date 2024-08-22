# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

script_t = record(
    prepare = field(str | Select | None, default = None),
    build = str | Select,
    install = str | Select,
    patches = field(list[str] | Select | None, default = None),
)

dep_t = record(
    name = str,
    source = str,
    paths = list[str],
)
