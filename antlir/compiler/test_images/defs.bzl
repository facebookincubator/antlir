# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# IMPORTANT: Add an ANTLIR_RULE_TEST shim below for any new loads.
load("//antlir/bzl:build_defs.bzl", "buck_genrule", "buck_sh_binary", "cpp_binary", "export_file", "python_binary")

#
# Why do we need these shims?
#
# `_assert_package` in `build_defs_impl.bzl` deliberately treats
# `compiler/test_images` as being outside of the Antlir codebase.
#
# This allows our collection of test images to validate that the Antlir
# user-instantiatable API is properly annotated with `antlir_rule` kwargs.
#
# However, this also means that we cannot directly use Buck rules as
# provided by `build_defs.bzl` inside `test_images`.  But all of these rule
# invocations still need to go through `build_defs.bzl` for FB/OSS
# compatibility.
#
# So, we shim the shim for the few rules that `test_images` rely on.  For
# these shimmed rules, `ANTLIR_RULE_TEST = "user-internal"` was picked
# arbitrarily -- "user-facing" would be fine too.  Internal seemed slightly
# preferable since it would expand CI coverage of Antlir changes.
#

ANTLIR_RULE_TEST = "user-internal"  # Explained above

def _buck_genrule(*args, **kwargs):
    buck_genrule(antlir_rule = ANTLIR_RULE_TEST, *args, **kwargs)

def _buck_sh_binary(*args, **kwargs):
    buck_sh_binary(antlir_rule = ANTLIR_RULE_TEST, *args, **kwargs)

def _export_file(*args, **kwargs):
    export_file(antlir_rule = ANTLIR_RULE_TEST, *args, **kwargs)

def _python_binary(*args, **kwargs):
    python_binary(antlir_rule = ANTLIR_RULE_TEST, *args, **kwargs)

def _cpp_binary(*args, **kwargs):
    cpp_binary(antlir_rule = ANTLIR_RULE_TEST, *args, **kwargs)

# Create a signed copy of the test RPM file passed in. It is signed with
# the key from //antlir/rpm:gpg-test-keypair.
#
# name: Target name.
# filename: RPM filename to pass to export_file() and sign.
#
def _sign_rpm_test_file(name, filename):
    _export_file(name = filename)

    _buck_genrule(
        name = name,
        out = "signed_test.rpm",
        bash = '''
        set -ue -o pipefail
        set -x
        TMPGNUPGHOME=\\$(mktemp -d)
        export GNUPGHOME=/tmp/\\$(basename $TMPGNUPGHOME)
        ln -s $TMPGNUPGHOME $GNUPGHOME
        cp $(location :{filename}) "$OUT"
        keypair_path=$(location //antlir/rpm/tests/gpg_test_keypair:gpg-test-keypair)
        gpg --import "$keypair_path/private.key"
        rpmsign --addsign --define="_gpg_name Test Key" "$OUT"
        '''.format(filename = filename),
    )

defs = struct(
    buck_genrule = _buck_genrule,
    buck_sh_binary = _buck_sh_binary,
    export_file = _export_file,
    python_binary = _python_binary,
    cpp_binary = _cpp_binary,
    sign_rpm_test_file = _sign_rpm_test_file,
)
