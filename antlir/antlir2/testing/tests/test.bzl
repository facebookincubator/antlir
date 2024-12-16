# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:internal_external.bzl", "internal_external")

def _product(*iterables):
    # product('ABCD', 'xy') → Ax Ay Bx By Cx Cy Dx Dy
    pools = [tuple(pool) for pool in iterables]

    result = [[]]
    for pool in pools:
        result = [x + [y] for x in result for y in pool]

    products = []
    for prod in result:
        products.append(tuple(prod))
    return products

def test_variants(
        *,
        test_rule,
        lang: str,
        layer: str = ":base",
        **kwargs):
    for (boot, user, rootless, os) in _product(
        (True, False, "wait-default"),
        ("root", "nobody"),
        (True, False),
        internal_external(
            fb = ("centos9", "centos10"),
            oss = ("centos9",),
        ),
    ):
        if rootless and boot:
            # TODO(T187078382): booted tests still must use
            # systemd-nspawn and are incompatible with rootless
            continue
        name_parts = (
            "test",
            lang,
            "boot" if boot else None,
            "requires_units" if boot == "wait-default" else None,
            user if user != "root" else None,
            "rootless" if rootless else None,
            os,
        )
        name = "-".join([n for n in name_parts if n])
        test_rule(
            name = name,
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
            rootless = rootless,
            default_os = os,
            **kwargs
        )
