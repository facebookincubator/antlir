# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

action_t = shape.shape(
    status = shape.enum("success", "notfound", "unavail", "tryagain"),
    action = shape.enum("return", "continue", "merge"),
)

service_t = shape.shape(
    name = str,
    action = shape.field(action_t, optional = True),
)

database_t = shape.shape(
    # not an exhaustive list, but does contain the things we care about
    name = shape.enum("group", "hosts", "passwd", "shadow"),
    services = shape.list(service_t),
)

conf_t = shape.shape(
    databases = shape.list(database_t),
)
