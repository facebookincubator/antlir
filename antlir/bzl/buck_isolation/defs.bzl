# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "buck_genrule", "is_buck2")

def buck_isolation_py(name):
    buck_genrule(
        name = name + "__dummy",
        bash = 'echo hello > "$OUT"',
    )

    bash = """\
cat > "$OUT" <<'EOF'
_IS_BUCK2 = {is_buck2}
_BUCK1_BASE_BUCK_OUT = {repr_buck1_base_buck_out}

# This relies on the fact that Buck fails to shell-quote macros. Q82995 e.g.
_PATH_IN_BUCK_OUT = '''\\
$(location :{name}__dummy)\\
'''


def is_buck_using_isolation():
    if _IS_BUCK2:
        return not _PATH_IN_BUCK_OUT.startswith('buck-out/v2/')
    return _BUCK1_BASE_BUCK_OUT != 'buck-out'
EOF
""".format(
        is_buck2 = repr(is_buck2()),
        name = name,
        repr_buck1_base_buck_out = repr(
            native.read_config("buck", "base_buck_out_dir", "buck-out"),
        ),
    )
    buck_genrule(name = name, bash = bash)
