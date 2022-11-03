# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "buck_genrule", "http_file")

def rustc_toolchain(
        version,
        channel,
        target,
        arch,
        sha256):
    full_target = "{}-{}".format(arch, target)
    download_name = "rust-{}__download".format(full_target)
    exploded_name = "rust-{}__exploded".format(full_target)
    toolchain_name = "rust-{}-toolchain".format(full_target)

    http_file(
        name = download_name,
        out = "rust.tar.gz",
        sha256 = sha256,
        urls = [
            "https://static.rust-lang.org/dist/{}/rust-{}-{}.tar.gz".format(version, channel, full_target),
        ],
        visibility = [],
    )

    buck_genrule(
        name = exploded_name,
        out = ".",
        cmd = """
            cd $OUT
            tar xvf $(location :{})
        """.format(download_name),
    )

    dir_name = "rust-{}-{}-{}".format(channel, arch, target)
    buck_genrule(
        name = toolchain_name,
        out = "run",
        cmd = """
cat > "$TMP/out" << 'EOF'
#!/bin/bash
set -ue -o pipefail -o noclobber
exec $(location :{exploded_name})/{dir_name}/rustc/bin/rustc \
    -L $(location :{exploded_name})/{dir_name}/rust-std-{arch}-{target}/lib/rustlib/{arch}-{target}/lib "$@"
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
        """.format(
            arch = arch,
            exploded_name = exploded_name,
            dir_name = dir_name,
            target = target,
        ),
        executable = True,
        visibility = ["PUBLIC"],
    )

    return toolchain_name
