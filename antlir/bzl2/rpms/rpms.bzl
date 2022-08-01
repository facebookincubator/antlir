# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:oss_shim.bzl", "is_buck2")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.shape.bzl", image_source_t = "target_tagged_image_source_t")
load("//antlir/bzl/image/feature:rpm_install_info_dummy_action_item.bzl", "RPM_INSTALL_INFO_DUMMY_ACTION_ITEM")
load("//antlir/bzl/image/feature:rpms.shape.bzl", "rpm_action_item_t")
load(
    "//antlir/bzl2:generate_feature_target_name.bzl",
    "generate_feature_target_name",
)
load(
    "//antlir/bzl2:image_source_helper.bzl",
    "mark_path",
    "normalize_target_and_mark_path_in_source_dict",
)
load("//antlir/bzl2:providers.bzl", "ItemInfo", "RpmInfo")

def _rpm_name_or_source(name_source):
    # Normal RPM names cannot have a colon, whereas target paths
    # ALWAYS have a colon. `image.source` is a struct.
    if not types.is_string(name_source) or ":" in name_source:
        return "source"
    else:
        return "name"

# It'd be a bit expensive to do any kind of validation of RPM
# names at this point, since we'd need the repo snapshot to decide
# whether the names are valid, and whether they contain a
# version or release number.  That'll happen later in the build.
def _generate_rpm_items(rpmlist, action, needs_version_set, flavors):
    flavors = flavors or []
    flavors_specified = bool(flavors)

    rpm_items = []
    if action == "install":
        # We have a dummy rpm so that we consider empty lists when
        # we check coverage of all flavors in a feature.
        #
        # ```
        # feature.new(
        #     name = "test",
        #     features=[
        #         feature.rpms_install([], flavors=["only-relevant-on-centos7"]),
        #         feature.rpms_install([], flavors=["only-relevant-on-centos8"]),
        #     ],
        #     flavors = ["centos7", "centos8"],
        # )
        # ```
        #
        # should not throw an error.
        rpm_items.append(
            rpm_action_item_t(
                name = RPM_INSTALL_INFO_DUMMY_ACTION_ITEM,
                action = action,
                flavor_to_version_set = {flavor: BZL_CONST.version_set_allow_all_versions for flavor in flavors},
                flavors_specified = flavors_specified,
            ),
        )

    deps = []
    for path in rpmlist:
        source = None
        name = None
        vs_name = None
        if _rpm_name_or_source(path) == "source":
            source_dict = shape.as_dict_shallow(image_source(path))
            source_dict, source_target = normalize_target_and_mark_path_in_source_dict(
                source_dict,
            )
            deps.append(source_target)
            source = shape.new(image_source_t, **source_dict)
        else:
            name = path
            if needs_version_set:
                vs_name = name

        flavor_to_version_set = {}
        for flavor in flavors or REPO_CFG.flavor_to_config.keys():
            vs_path_prefix = REPO_CFG.flavor_to_config[flavor].version_set_path

            if (vs_path_prefix != BZL_CONST.version_set_allow_all_versions and
                vs_name):
                rpm_target = vs_path_prefix + "/rpm:" + vs_name
                flavor_to_version_set[flavor] = mark_path(rpm_target)
                deps.append(rpm_target)
            else:
                flavor_to_version_set[flavor] = \
                    BZL_CONST.version_set_allow_all_versions

        rpm_items.append(shape.new(
            rpm_action_item_t,
            action = action,
            flavors_specified = flavors_specified,
            flavor_to_version_set = flavor_to_version_set,
            source = source,
            name = name,
        ))

    return struct(
        rpm_items = rpm_items,
        rpm_deps = deps,
    )

def _rpm_rule_impl(ctx):
    return [
        native.DefaultInfo(),
        ItemInfo(items = struct(**{"rpms": ctx.attrs.rpm_items})),
        RpmInfo(
            action = ctx.attrs.action,
            flavors = ctx.attrs.flavors,
        ),
    ]

_rpm_rule = native.rule(
    impl = _rpm_rule_impl,
    attrs = {
        "action": native.attrs.string(),
        "deps": native.attrs.list(native.attrs.dep(), default = []),

        # flavors specified in call to `feature.rpms_{install,remove_if_exists}`
        "flavors": native.attrs.list(native.attrs.string(), default = []),

        # gets serialized to json when `feature.new` is called and used as
        # kwargs in compiler `ItemFactory`
        "rpm_items": native.attrs.list(native.attrs.dict(native.attrs.string(), native.attrs.any())),

        # for query
        "type": native.attrs.string(default = "image_feature"),
    },
) if is_buck2() else None

def maybe_add_rpm_rule(
        name,
        rpm_items,
        flavors,
        include_in_target_name = None,
        deps = [],
        debug = False):
    key = "rpms"

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = rpm_items,
        include_in_name = include_in_target_name if debug else None,
    )

    if not native.rule_exists(target_name):
        _rpm_rule(
            name = target_name,
            action = name,
            rpm_items = [
                shape.as_serializable_dict(r)
                for r in rpm_items
            ],
            flavors = flavors,
            deps = deps,
        )

    return ":" + target_name

def rpms_install(rpmlist, flavors = None):
    """
    `feature.rpms_install(["foo"])` installs `foo.rpm`,
    `feature.rpms_install(["//target:bar"])` builds `bar` target and installs
    resulting RPM.

    The argument to both functions is a list of RPM package names to install,
    without version or release numbers. Dependencies are installed as needed.
    Order is not significant.

    As shown in the above example, RPMs may also be installed that are the
    outputs of another buck rule by providing a target path or an `image.source`
    (docs in`image_source.bzl`), or by directly providing a target path.

    If RPMs are specified by name, as in the first example above, the default
    behavior is to install the latest available version of the RPMs. Particular
    versions of RPMs can be pinned by specifying `image.opts` with
    `rpm_version_set_overrides` argument. This argument must be the list of
    structures defined by `image.rpm.nevra()`:

    ```
    image.layer(
        name = "my_layer",
        features = [
            feature.rpms_install([
                "foo",
            ]),
        ],
        flavor_config_override = image.opts(
            rpm_version_set_overrides = [
                image.rpm.nevra(
                    name = "foo",
                    epoch = "0",
                    version = "1",
                    release = "el7",
                    arch = "x86_64"
                ),
            ],
        ),
    )
    ```

    In this example `foo-1-el7.x86_64` will be installed into the layer
    `my_layer` even if a newer version is available.

    If the argument `rpmlist` lists both RPM name and buck rule targets, RPMs
    specified by buck rule targets are installed before RPMs specified by names.
    Hence, if an RPM defined by name requires a newer version of an RPM defined
    by buck rule target, the RPM will be upgraded and the whole operation may
    succeed. Thus, the explicit specification of RPM version by buck rule does
    not guarantee that this particular version is present in resulting image.

    Another important caveat about RPMs specified by buck rule targets is that
    downgrade is allowable: if the parent layer has RPM `foobar-v2` installed,
    and then `foobar-v1` is specified by a buck rule, the result of RPM
    installation will be `foobar-v2` downgraded to `foobar-v1`.

    `feature.rpms_install()` provides only limited support for RPM post-install
    scripts. Those scripts are executed in a virtual environment without runtime
    mounts like `/proc`. As an example, the script may invoke a binary requiring
    `/proc/self/exe` or a shared library from a directory not available in the
    image. Then the binary fails, and the final result of the operation would
    differ from the RPM installation on the host where the binary succeeds. The
    issue may be aggravated by the lack of error handling in the script making
    the RPM install operation successful even if the binary fails.
    """
    return _generate_rpm_items(
        rpmlist,
        "install",
        needs_version_set = True,
        flavors = flavors,
    )

def rpms_remove_if_exists(rpmlist, flavors = None):
    """
    `feature.rpms_remove_if_exists(["baz"])` removes `baz.rpm` if exists.

    Note that removals may only be applied against the parent layer -- if your
    current layer includes features both removing and installing the same
    package, this will cause a build failure.
    """
    return _generate_rpm_items(
        rpmlist,
        "remove_if_exists",
        needs_version_set = False,
        flavors = flavors,
    )

rpms = struct(
    install = rpms_install,
    remove_if_exists = rpms_remove_if_exists,
)
