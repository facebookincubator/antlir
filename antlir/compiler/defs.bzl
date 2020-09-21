load("//antlir/bzl:artifacts_require_repo.bzl", "ARTIFACTS_REQUIRE_REPO")
load("//antlir/bzl:constants.bzl", "VERSION_SET_ALLOW_ALL_VERSIONS")
load("//antlir/bzl:oss_shim.bzl", "python_unittest")
load("//antlir/bzl/image_actions:feature.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")

TEST_IMAGE_PREFIX = "//antlir/compiler/test_images:"

def READ_MY_DOC_image_feature_target(name):
    """
    DANGER: If you depend on a feature target for testing, you MUST manually
    add any `image_feature` targets that it depends on to your `deps = []`.
    If you fail to do this, Buck will not know to rebuild the the test if
    one of its indirect `image_feature` dependencies changes.  See
    `image_feature.py` for an explanation.
    """
    return PRIVATE_DO_NOT_USE_feature_target_name(
        name = name,
        version_set = VERSION_SET_ALLOW_ALL_VERSIONS,
    )

def image_feature_python_unittest(test_image_feature_transitive_deps, deps = None, env = None, **kwargs):
    env = env or {}
    env.update({
        "test_image_feature_path_to_" + t: "$(location {})".format(
            TEST_IMAGE_PREFIX + t,
        )
        for t in test_image_feature_transitive_deps
    })
    env["test_image_feature_built_artifacts_require_repo"] = \
        str(int(ARTIFACTS_REQUIRE_REPO))

    deps = (deps or []) + [":sample_items"]

    # For now cpp_deps is raw buck deps for python_ targets
    cpp_deps = [
        TEST_IMAGE_PREFIX + t
        for t in test_image_feature_transitive_deps
    ]
    return python_unittest(
        env = env,
        # The test reads `feature.json`, so we need actual files on disk.
        par_style = "zip",
        deps = deps,
        cpp_deps = cpp_deps,
        **kwargs
    )
