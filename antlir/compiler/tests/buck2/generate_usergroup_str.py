#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


def generate_group_str(
    group_name="",
    password="",
    gid="",
    user_list=None,
):
    user_list = user_list if user_list else []
    return ":".join([group_name, password, gid, ",".join(user_list)])


def generate_user_str(
    user_name="",
    password="",
    uid="",
    gid="",
    comment="",
    home_dir="",
    shell="",
):
    return ":".join([user_name, password, uid, gid, comment, home_dir, shell])
