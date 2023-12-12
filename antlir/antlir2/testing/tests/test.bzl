# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def test_variants(
        *,
        test_rule,
        lang: str,
        layer: str = ":base",
        **kwargs):
    for boot in [True, False, "wait-default"]:
        name = "test-" + lang + ("-booted" if boot else "")
        name = name + ("-requires-units" if boot == "wait-default" else "")
        for user in ["root", "nobody"]:
            test_rule(
                name = name + ("-" + user if user != "root" else ""),
                boot = bool(boot),
                boot_requires_units = ["default.target"] if boot == "wait-default" else None,
                run_as_user = user,
                layer = layer,
                env = {
                    "ANTLIR2_TEST": "1",
                    "BOOT": str(boot),
                    "JSON_ENV": '{"foo": "bar"}',
                    "TEST_USER": user,
                },
                **kwargs
            )
