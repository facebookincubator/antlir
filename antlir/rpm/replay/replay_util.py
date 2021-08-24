# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


def make_target_path_map(target_path_pairs):
    assert len(target_path_pairs) % 2 == 0, f"Odd-length {target_path_pairs}"
    it = iter(target_path_pairs)
    return dict(zip(it, it))
