# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl?v2_only", antlir2_image_rust_test = "image_rust_test")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":container_opts.bzl", "normalize_container_opts")

def image_rust_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        hostname = None,
        container_opts = None,
        visibility = None,
        antlir2 = None,
        **rust_unittest_kwargs):
    container_opts = normalize_container_opts(container_opts)
    if antlir2_shim.upgrade_or_shadow_test(
        antlir2 = antlir2,
        fn = antlir2_image_rust_test,
        name = name,
        layer = layer,
        boot = boot,
        run_as_user = run_as_user,
        boot_requires_units = ["dbus.socket"] if (boot and container_opts and container_opts.boot_await_dbus) else [],
        crate = rust_unittest_kwargs.pop("crate", name + "_unittest"),
        hostname = hostname,
        **rust_unittest_kwargs
    ) != "upgrade":
        fail("antlir1 is dead")
