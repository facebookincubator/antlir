# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl?v2_only", antlir2_image_python_test = "image_python_test")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":container_opts.bzl", "normalize_container_opts")
load(":flavor.shape.bzl", "flavor_t")
load(":types.bzl", "types")

# This exists to hack around a complex FB-internal migration. *sigh*
# It should be removable when this is done:  https://fburl.com/nxc3u5mk
_TEMP_TP_TAG = "use-testpilot-adapter"

_OPTIONAL_STRUCT = types.optional(types.struct)

types.lint_noop(flavor_t, _OPTIONAL_STRUCT)

def image_python_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        visibility = None,
        par_style = None,
        hostname = None,
        container_opts = None,
        flavor = None,
        flavor_config_override: _OPTIONAL_STRUCT = None,
        antlir2 = None,
        **python_unittest_kwargs):
    visibility = visibility or []
    container_opts = normalize_container_opts(container_opts)

    if antlir2_shim.upgrade_or_shadow_test(
        antlir2 = antlir2,
        fn = antlir2_image_python_test,
        name = name,
        layer = layer + ".antlir2",
        boot = boot,
        run_as_user = run_as_user,
        boot_requires_units = ["dbus.socket"] if (boot and container_opts and container_opts.boot_await_dbus) else [],
        hostname = hostname,
        fake_buck1 = struct(
            fn = antlir2_shim.fake_buck1_test,
            name = name,
            test = "python",
        ),
        **python_unittest_kwargs
    ) != "upgrade":
        fail("antlir1 is dead")
        return
