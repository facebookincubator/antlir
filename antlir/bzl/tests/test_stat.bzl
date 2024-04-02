# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:stat.bzl", "stat")

def assert_eq(actual, expected, sym = None):
    if actual != expected:
        fail("assert_eq failed: {} != {}".format(actual, expected) + (" ({})".format(sym) if sym else ""))

def test_simple_parse():
    assert_eq(0o400, stat.parse("u+r"))
    assert_eq(0o440, stat.parse("ug+r"))
    assert_eq(0o444, stat.parse("ugo+r"))
    assert_eq(0o444, stat.parse("a+r"))
    assert_eq(0o555, stat.parse("a+rx"))

def test_parse():
    # generated with:
    # with tempfile.NamedTemporaryFile() as tf:
    # for s in inputs:
    #     subprocess.run(["chmod", f"a-rwxXst,{s}", tf.name])
    #     st = os.stat(tf.name)
    #     mode = stat.S_IMODE(st.st_mode)
    #     print(f'"{s}": {mode}, # 0o{mode:o}')
    cases = {
        "+t": 512,  # 0o1000
        "+tx": 585,  # 0o1111
        "+x": 73,  # 0o111
        "a+r,a+w,u+x": 502,  # 0o766
        "a+r,o+w": 294,  # 0o446
        "a+rx,u+w": 493,  # 0o755
        "a+srx": 3437,  # 0o6555
        "a+t,a+r": 804,  # 0o1444
        "a+wrx": 511,  # 0o777
        "g+sw": 1040,  # 0o2020
        "u+s,g+s": 3072,  # 0o6000
        "u+s,g+s,a+s": 3072,  # 0o6000
        "u+sr": 2304,  # 0o4400
        "u+t,a+r": 292,  # 0o444
        "u+w,a+xr": 493,  # 0o755
        "u+wrx,g+xrw,o+rwx": 511,  # 0o777
        "u+wrx,g+xrw,o+rwx,a+r": 511,  # 0o777
        "u+wrx,og+r": 484,  # 0o744
        "u+wrx,og+xrw": 511,  # 0o777
        "u+x": 64,  # 0o100
        "ug+s": 3072,  # 0o6000
        "ug+s,a+trx": 3949,  # 0o7555
        "uog+w": 146,  # 0o222
    }
    for sym, expected in cases.items():
        actual = stat.parse(sym)
        assert_eq(
            actual,
            expected,
            sym,
        )
