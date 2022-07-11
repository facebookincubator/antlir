#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.tests.layer_resource import layer_resource, LAYER_SLASH_ENCODE


def get_layer_by_prefix(resources, target_resource_prefix):
    for target in resources:
        if target.startswith(target_resource_prefix):
            return target[len(target_resource_prefix) :]
    raise RuntimeError("layer resource undefined")


def get_layer_target_to_path_by_prefix(
    resources, package, target_resource_prefix
):
    return {
        target[len(target_resource_prefix) :]: path
        for target, path in [
            (
                target.replace(LAYER_SLASH_ENCODE, "/"),
                str(layer_resource(package, target)),
            )
            for target in resources
            if target.startswith(target_resource_prefix)
        ]
    }


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
