# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_genrule")

def _get_build_info():
    return struct(
        package_name = native.read_config("build_info", "package_name"),
        package_version = native.read_config("build_info", "package_version"),
    )

def initrd_release(name):
    info = _get_build_info()

    version = info.package_version if info.package_version else "local"
    build_id = "{}:{}".format(info.package_name, info.package_version) if info.package_version else "local"

    buck_genrule(
        name = name,
        out = "initrd-release",
        cmd = """
            echo "NAME='Antlir Linux'" > $OUT
            echo "ID='antlir-linux'" >> $OUT
            echo "VERSION='{version}'" >> $OUT
            echo "PRETTY_NAME='Antlir Linux Initrd ({version})'" >> $OUT
            echo "BUILD_ID='{build_id}'" >> $OUT
            echo "VARIANT='Initrd'" >> $OUT
            echo "VARIANT_ID='initrd'" >> $OUT
            echo "ANSI_COLOR='0;34'" >> $OUT
        """.format(
            version = version,
            build_id = build_id,
        ),
    )
