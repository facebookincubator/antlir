# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:oss_shim.bzl", "python_unittest")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl/image/feature:install.bzl", "TEST_ONLY_wrap_buck_runnable")
load("//antlir/bzl/image/feature:new.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")

TEST_IMAGE_PREFIX = antlir_dep("compiler/test_images:")

def READ_MY_DOC_image_feature_target(name):
    """
    DANGER: If you depend on a feature target for testing, you MUST manually
    add any `feature` targets that it depends on to your `deps = []`.
    If you fail to do this, Buck will not know to rebuild the the test if
    one of its indirect `feature` dependencies changes.  See
    `feature/new.bzl` for an explanation.
    """
    return PRIVATE_DO_NOT_USE_feature_target_name(name)

def image_feature_python_unittest(
        test_image_feature_transitive_deps,
        test_image_feature_wrapped_transitive_deps,
        deps = None,
        env = None,
        **kwargs):
    env = env or {}
    env.update({
        "test_image_feature_path_to_" + t: "$(location {})".format(
            TEST_IMAGE_PREFIX + t,
        )
        for t in test_image_feature_transitive_deps
    })

    env.update({
        "test_image_feature_path_to_" + TEST_ONLY_wrap_buck_runnable(TEST_IMAGE_PREFIX + t, path): "$(location {})".format(
            TEST_IMAGE_PREFIX + TEST_ONLY_wrap_buck_runnable(TEST_IMAGE_PREFIX + t, path),
        )
        for t, path in test_image_feature_wrapped_transitive_deps
    })

    env["test_image_feature_built_artifacts_require_repo"] = \
        str(int(REPO_CFG.artifacts_require_repo))

    deps = (deps or []) + [":sample_items"]

    # For now cpp_deps is raw buck deps for python_ targets
    cpp_deps = [
        TEST_IMAGE_PREFIX + t
        for t in test_image_feature_transitive_deps
    ]
    return python_unittest(
        env = env,
        deps = deps,
        cpp_deps = cpp_deps,
        **kwargs
    )
