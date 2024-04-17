# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

status_t = shape.enum("success", "notfound", "unavail", "tryagain")
action_enum_t = shape.enum("return", "continue", "merge")

action_t = shape.shape(
    status = status_t,
    action = action_enum_t,
)

service_t = shape.shape(
    name = str,
    action = shape.field(action_t, optional = True),
)

# not an exhaustive list, but does contain the things we care about
database_name_t = shape.enum("group", "hosts", "passwd", "shadow")

database_t = shape.shape(
    name = database_name_t,
    services = shape.list(service_t),
)

conf_t = shape.shape(
    databases = shape.list(database_t),
)
