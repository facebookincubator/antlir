# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:oss_shim.bzl", "buck_command_alias", "buck_genrule", "buck_sh_test")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def shape(name, thrift):
    thrift = normalize_target(maybe_export_file(thrift))
    thrift_compiler = native.read_config("thrift", "compiler", None)
    buck_genrule(
        name = name + "--thrift.json",
        out = "thrift.json",
        cmd = """
            cd $TMP
            $(exe {compiler}) --gen json_experimental $(location {thrift})
            mv gen-json_experimental/*.json $OUT
        """.format(compiler = thrift_compiler, thrift = thrift),
    )

    # For completely new shape files, the bzl file will not exist, so it's hard
    # to generate the rest of these rules.
    # This is rare enough that we can just ask the user nicely to make a new
    # empty file.
    if not native.glob([name]):
        fail("new shape files must be created manually, please create an empty file '{}'".format(
            name,
        ))

    out = normalize_target(maybe_export_file(name))
    generator = normalize_target("//antlir/bzl/shape:bzl-codegen")

    # easy alias per-shape to update the generated code
    buck_command_alias(
        name = name + "-update",
        args = ["$(location :{}--thrift.json)".format(name), "$(location {})".format(out)],
        exe = generator,
        labels = ["shape-update"],
    )
    update = normalize_target(":{}-update".format(name))

    # create a buck_genrule that always outputs a fresh bzl file so that it can
    # be diffed for test purposes
    buck_genrule(
        name = name + "-gen",
        out = name + ".bzl",
        cmd = "$(exe {}) $(location :{}--thrift.json) $OUT".format(generator, name),
    )

    buck_sh_test(
        name = name + "-up-to-date",
        args = ["$(location {})".format(out), "$(location :{}-gen)".format(name), "buck run {}".format(update)],
        test = "fbcode//antlir/bzl/shape:test-up-to-date",
    )
