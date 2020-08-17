load("//fs_image/bzl:oss_shim.bzl", "buck_genrule", "export_file")

# Create a signed copy of the test RPM file passed in. It is signed with
# the key from //fs_image/rpm:gpg-test-keypair.
#
# name: Target name.
# filename: RPM filename to pass to export_file() and sign.
#
def sign_rpm_test_file(name, filename):
    export_file(name = filename)

    buck_genrule(
        name = name,
        out = "signed_test.rpm",
        bash = '''
        set -ue -o pipefail
        export GNUPGHOME=\\$(mktemp -d)
        cp $(location :{filename}) "$OUT"
        keypair_path=$(location //fs_image/rpm:gpg-test-keypair)
        gpg --import "$keypair_path/private.key"
        rpmsign --addsign --define="_gpg_name Test Key" "$OUT"
        '''.format(filename = filename),
    )
