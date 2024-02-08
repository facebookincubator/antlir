# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

#
# Prefer to keep this default-initializable to avoid having to update a
# bunch of tests and other Python callsites.
container_opts_t = shape.shape(
    # Future: move `boot` in here, too.
    boot_await_dbus = shape.field(bool, default = True),
    boot_await_system_running = shape.field(bool, default = False),
)
