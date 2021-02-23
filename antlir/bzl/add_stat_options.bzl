# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

mode_t = shape.union_t(int, str)

def add_stat_options(d, mode, user, group):
    if mode != None:
        d["mode"] = mode
    if user != None or group != None:
        if user == None:
            user = "root"
        if group == None:
            group = "root"
        d["user_group"] = "{}:{}".format(user, group)
