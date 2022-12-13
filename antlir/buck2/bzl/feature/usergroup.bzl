# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def user_add(
        *,
        username: str.type,
        primary_group: str.type,
        home_dir: str.type,
        shell: str.type = SHELL_NOLOGIN,
        uid: [int.type, None] = None,
        supplementary_groups: [str.type] = [],
        comment: [str.type, None] = None) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "user",
        kwargs = {
            "comment": comment,
            "home_dir": home_dir,
            "primary_group": primary_group,
            "shell": shell,
            "supplementary_groups": supplementary_groups,
            "uid": uid,
            "username": username,
        },
    )

def group_add(
        *,
        groupname: str.type,
        gid: [int.type, None] = None) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "group",
        kwargs = {
            "gid": gid,
            "groupname": groupname,
        },
    )

def usermod(
        *,
        username: str.type,
        add_supplementary_groups: [[str.type], None] = None) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "usermod",
        kwargs = {
            "add_supplementary_groups": add_supplementary_groups or [],
            "username": username,
        },
    )

def user_to_json(
        username: str.type,
        uid: [int.type, None],
        home_dir: str.type,
        primary_group: str.type,
        supplementary_groups: [str.type],
        shell: str.type,
        comment: [str.type, None],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "comment": comment,
        "home_dir": home_dir,
        "name": username,
        "primary_group": primary_group,
        "shell": shell,
        "supplementary_groups": supplementary_groups,
        "uid": uid,
    }

def group_to_json(
        groupname: str.type,
        gid: [int.type, None],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "gid": gid,
        "name": groupname,
    }

def usermod_to_json(
        username: str.type,
        add_supplementary_groups: [str.type],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "add_supplementary_groups": add_supplementary_groups,
        "username": username,
    }
